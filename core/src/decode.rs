//! Decodage d'une photo en 32 x 32 niveaux de gris, avec ses dimensions d'origine.
//! Sous Windows par WIC, le composant d'images du systeme, ailleurs par la crate image.

use crate::phash::SIDE;

pub struct Small {
    pub width: u32,
    pub height: u32,
    pub px: [u8; SIDE * SIDE],
}

#[cfg(windows)]
pub use wic::small;

#[cfg(not(windows))]
pub use portable::small;

#[cfg(windows)]
mod wic {
    use super::Small;
    use crate::phash::SIDE;
    use std::io;
    use std::path::Path;
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Graphics::Imaging::*;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use windows::core::{HSTRING, Interface};

    thread_local! {
        // Une fabrique par fil de calcul, COM initialise en multithread
        static FACTORY: windows::core::Result<IWICImagingFactory> = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
        };
    }

    pub fn small(path: &Path) -> io::Result<Small> {
        FACTORY.with(|factory| {
            let f = factory.as_ref().map_err(|e| io::Error::from(e.clone()))?;
            unsafe {
                let decoder = f.CreateDecoderFromFilename(
                    &HSTRING::from(path),
                    None,
                    GENERIC_READ,
                    WICDecodeMetadataCacheOnDemand,
                )?;
                let frame = decoder.GetFrame(0)?;
                let (mut width, mut height) = (0, 0);
                frame.GetSize(&mut width, &mut height)?;
                let source = match reduced(f, &frame) {
                    Ok(s) => s,
                    Err(_) => frame.cast()?,
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

    /// Image decodee directement a la plus petite taille que le decodeur sait produire :
    /// un JPEG au huitieme, sans calculer ses pixels en pleine definition
    unsafe fn reduced(
        f: &IWICImagingFactory,
        frame: &IWICBitmapFrameDecode,
    ) -> windows::core::Result<IWICBitmapSource> {
        unsafe {
            let transform: IWICBitmapSourceTransform = frame.cast()?;
            let (mut w, mut h) = (SIDE as u32, SIDE as u32);
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
}
