//! Diagnostic du decodage WIC d'une photo : taille et format que le decodeur propose en
//! decodage reduit, temps du decodage reduit et du decodage en pleine definition.
//!
//! cargo run --release -p jumelles-core --example wic -- <photo>

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows seulement");
}

#[cfg(windows)]
fn main() {
    use std::time::Instant;
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Graphics::Imaging::*;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use windows::core::{HSTRING, Interface};

    let path = std::env::args().nth(1).expect("photo");
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).unwrap();
        let frame = || {
            let d = f
                .CreateDecoderFromFilename(
                    &HSTRING::from(path.as_str()),
                    None,
                    GENERIC_READ,
                    WICDecodeMetadataCacheOnDemand,
                )
                .unwrap();
            d.GetFrame(0).unwrap()
        };
        let fr = frame();
        let (mut w, mut h) = (0, 0);
        fr.GetSize(&mut w, &mut h).unwrap();
        println!("taille : {w} x {h}");
        match fr.cast::<IWICBitmapSourceTransform>() {
            Err(e) => println!("IWICBitmapSourceTransform absent : {e}"),
            Ok(t) => {
                let (mut tw, mut th) = (32u32, 32u32);
                println!(
                    "GetClosestSize(32, 32) : {:?} -> {tw} x {th}",
                    t.GetClosestSize(&mut tw, &mut th)
                );
                let mut format = GUID_WICPixelFormat8bppGray;
                println!(
                    "GetClosestPixelFormat(8bppGray) : {:?}",
                    t.GetClosestPixelFormat(&mut format)
                );
                let gris = format == GUID_WICPixelFormat8bppGray;
                println!(
                    "gris natif : {gris}, 24bppBGR : {}",
                    format == GUID_WICPixelFormat24bppBGR
                );
                let bytes = if gris { 1 } else { 3 };
                for _ in 0..3 {
                    let t0 = Instant::now();
                    let t = frame().cast::<IWICBitmapSourceTransform>().unwrap();
                    let mut buf = vec![0u8; (tw * th * bytes) as usize];
                    let r = t.CopyPixels(
                        std::ptr::null(),
                        tw,
                        th,
                        &format,
                        WICBitmapTransformRotate0,
                        tw * bytes,
                        &mut buf,
                    );
                    println!(
                        "decodage reduit : {r:?} en {:.1} ms",
                        t0.elapsed().as_secs_f64() * 1000.0
                    );
                }
            }
        }
        for _ in 0..3 {
            let t0 = Instant::now();
            let fr = frame();
            let mut buf = vec![0u8; (w * h * 3) as usize];
            let conv = f.CreateFormatConverter().unwrap();
            conv.Initialize(
                &fr,
                &GUID_WICPixelFormat24bppBGR,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .unwrap();
            conv.CopyPixels(std::ptr::null(), w * 3, &mut buf).unwrap();
            println!("decodage complet : {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);
        }
        for _ in 0..3 {
            let t0 = Instant::now();
            let _ = jumelles_core::decode::small(std::path::Path::new(&path)).unwrap();
            println!("decode::small : {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);
        }
    }
}
