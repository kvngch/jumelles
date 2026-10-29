//! Decodage d'une photo en 32 x 32 niveaux de gris, avec ses dimensions d'origine.
//! Sous Windows par WIC, le composant d'images du systeme, ailleurs par la crate image.

use crate::phash::SIDE;

pub struct Small {
    pub width: u32,
    pub height: u32,
    pub px: [u8; SIDE * SIDE],
}

/// `small` : 32 x 32 niveaux de gris pour l'empreinte. `thumbnail` : vignette JPEG pour
/// l'ecran de tri, cote le plus long `max` pixels au plus, tournee selon l'orientation
/// EXIF de la photo.
#[cfg(windows)]
pub use wic::{small, thumbnail};

#[cfg(not(windows))]
pub use portable::{small, thumbnail};

#[cfg(windows)]
mod wic {
    use super::Small;
    use crate::phash::SIDE;
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use windows::Win32::Graphics::Imaging::*;
    use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, STREAM_SEEK_END, STREAM_SEEK_SET,
    };
    use windows::Win32::System::Variant::VT_UI2;
    use windows::Win32::UI::Shell::SHCreateMemStream;
    use windows::core::{Interface, w};

    thread_local! {
        // Une fabrique par fil de calcul, COM initialise en multithread
        static FACTORY: windows::core::Result<IWICImagingFactory> = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
        };
    }

    /// Decodeur de la photo. Le fichier est ouvert par Rust, qui passe la limite de 260
    /// caracteres des chemins, et doit rester ouvert tant que le decodeur sert.
    unsafe fn open(f: &IWICImagingFactory, path: &Path) -> io::Result<(File, IWICBitmapDecoder)> {
        let file = File::open(path)?;
        let decoder = unsafe {
            f.CreateDecoderFromFileHandle(
                file.as_raw_handle() as usize,
                std::ptr::null(),
                WICDecodeMetadataCacheOnDemand,
            )?
        };
        Ok((file, decoder))
    }

    pub fn small(path: &Path) -> io::Result<Small> {
        FACTORY.with(|factory| {
            let f = factory.as_ref().map_err(|e| io::Error::from(e.clone()))?;
            unsafe {
                let (_file, decoder) = open(f, path)?;
                let frame = decoder.GetFrame(0)?;
                let (mut width, mut height) = (0, 0);
                frame.GetSize(&mut width, &mut height)?;
                let source = match reduced(f, &frame, SIDE as u32, SIDE as u32) {
                    Ok(s) => s,
                    Err(_) => converted(f, &frame, &GUID_WICPixelFormat8bppGray)?,
                };
                let scaler = f.CreateBitmapScaler()?;
                scaler.Initialize(&source, SIDE as u32, SIDE as u32, WICBitmapInterpolationModeFant)?;
                let gray = f.CreateFormatConverter()?;
                gray.Initialize(
                    &scaler,
                    &GUID_WICPixelFormat8bppGray,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )?;
                let mut px = [0u8; SIDE * SIDE];
                gray.CopyPixels(std::ptr::null(), SIDE as u32, &mut px)?;
                Ok(Small { width, height, px })
            }
        })
    }

    pub fn thumbnail(path: &Path, max: u32) -> io::Result<Vec<u8>> {
        FACTORY.with(|factory| {
            let f = factory.as_ref().map_err(|e| io::Error::from(e.clone()))?;
            unsafe {
                let (_file, decoder) = open(f, path)?;
                let frame = decoder.GetFrame(0)?;
                let (mut w, mut h) = (0, 0);
                frame.GetSize(&mut w, &mut h)?;
                let scale = (max as f64 / w.max(h).max(1) as f64).min(1.0);
                let (tw, th) = (((w as f64 * scale) as u32).max(1), ((h as f64 * scale) as u32).max(1));
                let source = match reduced(f, &frame, tw, th) {
                    Ok(s) => s,
                    Err(_) => converted(f, &frame, &GUID_WICPixelFormat24bppBGR)?,
                };
                let scaler = f.CreateBitmapScaler()?;
                scaler.Initialize(&source, tw, th, WICBitmapInterpolationModeFant)?;
                let rotator = f.CreateBitmapFlipRotator()?;
                rotator.Initialize(&scaler, orientation(&frame))?;
                let bgr = f.CreateFormatConverter()?;
                bgr.Initialize(
                    &rotator,
                    &GUID_WICPixelFormat24bppBGR,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )?;
                jpeg(f, &bgr)
            }
        })
    }

    /// Image decodee directement a la plus petite taille que le decodeur sait produire en
    /// gardant au moins le double de la taille voulue : un JPEG au huitieme pour
    /// l'empreinte, sans calculer ses pixels en pleine definition
    unsafe fn reduced(
        f: &IWICImagingFactory,
        frame: &IWICBitmapFrameDecode,
        width: u32,
        height: u32,
    ) -> windows::core::Result<IWICBitmapSource> {
        unsafe {
            let transform: IWICBitmapSourceTransform = frame.cast()?;
            let (mut w, mut h) = (width * 2, height * 2);
            transform.GetClosestSize(&mut w, &mut h)?;
            let mut format = GUID_WICPixelFormat8bppGray;
            transform.GetClosestPixelFormat(&mut format)?;
            let bytes = match format {
                f if f == GUID_WICPixelFormat8bppGray => 1,
                f if f == GUID_WICPixelFormat24bppBGR => 3,
                _ => return Err(windows::core::Error::empty()),
            };
            let stride = w * bytes;
            let mut buf = vec![0u8; (stride * h) as usize];
            transform.CopyPixels(
                std::ptr::null(),
                w,
                h,
                &format,
                WICBitmapTransformRotate0,
                stride,
                &mut buf,
            )?;
            f.CreateBitmapFromMemory(w, h, &format, stride, &buf)?.cast()
        }
    }

    /// Image en pleine definition, au format voulu. Un IWICBitmapScaler pose sur l'image
    /// confie la reduction au decodeur s'il sait la faire : celui des HEIF reduit sans
    /// moyenner les pixels, et l'empreinte en sort crenelee (microsoft/WindowsAppSDK#5160).
    /// L'image d'un tel decodeur est donc copiee en memoire, et la reduction reste au Fant
    /// de WIC.
    unsafe fn converted(
        f: &IWICImagingFactory,
        frame: &IWICBitmapFrameDecode,
        format: &windows::core::GUID,
    ) -> windows::core::Result<IWICBitmapSource> {
        unsafe {
            let c = f.CreateFormatConverter()?;
            c.Initialize(
                frame,
                format,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )?;
            if frame.cast::<IWICBitmapSourceTransform>().is_ok() {
                return f.CreateBitmapFromSource(&c, WICBitmapCacheOnLoad)?.cast();
            }
            c.cast()
        }
    }

    /// Rotation ou miroir qui remet la photo droite, d'apres System.Photo.Orientation
    unsafe fn orientation(frame: &IWICBitmapFrameDecode) -> WICBitmapTransformOptions {
        let exif = unsafe {
            (|| -> windows::core::Result<u16> {
                let reader = frame.GetMetadataQueryReader()?;
                let mut v = PROPVARIANT::default();
                reader.GetMetadataByName(w!("System.Photo.Orientation"), &mut v)?;
                let o = if v.Anonymous.Anonymous.vt == VT_UI2 {
                    v.Anonymous.Anonymous.Anonymous.uiVal
                } else {
                    1
                };
                let _ = PropVariantClear(&mut v);
                Ok(o)
            })()
        };
        let both = |a: WICBitmapTransformOptions, b: WICBitmapTransformOptions| WICBitmapTransformOptions(a.0 | b.0);
        match exif.unwrap_or(1) {
            2 => WICBitmapTransformFlipHorizontal,
            3 => WICBitmapTransformRotate180,
            4 => WICBitmapTransformFlipVertical,
            5 => both(WICBitmapTransformRotate90, WICBitmapTransformFlipHorizontal),
            6 => WICBitmapTransformRotate90,
            7 => both(WICBitmapTransformRotate270, WICBitmapTransformFlipHorizontal),
            8 => WICBitmapTransformRotate270,
            _ => WICBitmapTransformRotate0,
        }
    }

    /// Encode en JPEG dans un flux en memoire
    unsafe fn jpeg(f: &IWICImagingFactory, src: &IWICFormatConverter) -> io::Result<Vec<u8>> {
        unsafe {
            let stream = SHCreateMemStream(None).ok_or_else(|| io::Error::other("flux en memoire"))?;
            let encoder = f.CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())?;
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let (mut frame, mut options) = (None, None);
            encoder.CreateNewFrame(&mut frame, &mut options)?;
            let frame: IWICBitmapFrameEncode = frame.ok_or_else(|| io::Error::other("image JPEG"))?;
            frame.Initialize(options.as_ref())?;
            let (mut w, mut h) = (0, 0);
            src.GetSize(&mut w, &mut h)?;
            frame.SetSize(w, h)?;
            let mut format = GUID_WICPixelFormat24bppBGR;
            frame.SetPixelFormat(&mut format)?;
            frame.WriteSource(src, std::ptr::null())?;
            frame.Commit()?;
            encoder.Commit()?;
            let mut size = 0u64;
            stream.Seek(0, STREAM_SEEK_END, Some(&mut size))?;
            stream.Seek(0, STREAM_SEEK_SET, None)?;
            let mut buf = vec![0u8; size as usize];
            let mut read = 0u32;
            stream
                .Read(buf.as_mut_ptr().cast(), size as u32, Some(&mut read))
                .ok()?;
            buf.truncate(read as usize);
            Ok(buf)
        }
    }
}

#[cfg(not(windows))]
mod portable {
    use super::Small;
    use crate::phash::SIDE;
    use std::io;
    use std::path::Path;

    pub fn small(path: &Path) -> io::Result<Small> {
        let img = image::open(path).map_err(io::Error::other)?;
        let gray = image::imageops::thumbnail(&img.to_luma8(), SIDE as u32, SIDE as u32);
        let px = gray
            .into_raw()
            .try_into()
            .map_err(|_| io::Error::other("vignette mal formee"))?;
        Ok(Small {
            width: img.width(),
            height: img.height(),
            px,
        })
    }

    pub fn thumbnail(path: &Path, max: u32) -> io::Result<Vec<u8>> {
        let mut decoder = image::ImageReader::open(path)?
            .into_decoder()
            .map_err(io::Error::other)?;
        let orientation = image::ImageDecoder::orientation(&mut decoder).map_err(io::Error::other)?;
        let mut img = image::DynamicImage::from_decoder(decoder).map_err(io::Error::other)?;
        img.apply_orientation(orientation);
        let mut out = Vec::new();
        img.thumbnail(max, max)
            .to_rgb8()
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85))
            .map_err(io::Error::other)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JPEG de 400 x 300, avec une orientation EXIF si elle est donnee
    fn jpeg(path: &std::path::Path, exif: Option<u8>) {
        let img = image::RgbImage::from_fn(400, 300, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]));
        let mut out = Vec::new();
        img.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90))
            .unwrap();
        if let Some(o) = exif {
            // segment APP1 minimal : en-tete TIFF petit-boutiste, une seule entree Orientation (0x0112)
            let mut app1 = vec![0xFF, 0xE1, 0x00, 0x22];
            app1.extend_from_slice(b"Exif\0\0II\x2A\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0");
            app1.extend_from_slice(&[o, 0, 0, 0, 0, 0, 0, 0]);
            out.splice(2..2, app1);
        }
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn vignettes() {
        let d = std::env::temp_dir().join(format!("jumelles-vignettes-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        jpeg(&d.join("droite.jpg"), None);
        jpeg(&d.join("tournee.jpg"), Some(6));
        let dims = |p: &str| {
            let v = thumbnail(&d.join(p), 200).unwrap();
            let img = image::load_from_memory_with_format(&v, image::ImageFormat::Jpeg).unwrap();
            (img.width(), img.height())
        };
        assert_eq!(dims("droite.jpg"), (200, 150));
        assert_eq!(dims("tournee.jpg"), (150, 200));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn chemin_de_plus_de_260_caracteres() {
        let racine = std::env::temp_dir().join(format!("jumelles-chemin-{}", std::process::id()));
        let mut d = racine.clone();
        while d.as_os_str().len() < 300 {
            d.push("un dossier au nom assez long");
        }
        std::fs::create_dir_all(&d).unwrap();
        jpeg(&d.join("photo.jpg"), None);
        assert_eq!(small(&d.join("photo.jpg")).unwrap().width, 400);
        assert!(thumbnail(&d.join("photo.jpg"), 100).is_ok());
        std::fs::remove_dir_all(racine).unwrap();
    }
}
