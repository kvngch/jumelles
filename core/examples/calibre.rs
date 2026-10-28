//! Calibration des seuils sur un dossier de photos reelles. Pour chaque photo, distances
//! avec ses variantes (reduite et recompressee, noir et blanc, eclaircie, recadree, avec
//! filigrane, tournee, en miroir), puis nombre de paires de photos differentes qui
//! passeraient les seuils.
//!
//! cargo run --release -p jumelles-core --example calibre -- <dossier de photos jpeg>

use image::{DynamicImage, ImageFormat, imageops};
use jumelles_core::phash::{self, Hashes, SIDE};
use rayon::prelude::*;

/// (grossiere, fine) en orientation d'origine, puis meilleure des huit orientations
type Distances = ((u32, u32), (u32, u32));

fn hash(img: &DynamicImage) -> Hashes {
    let g = imageops::thumbnail(&img.to_luma8(), SIDE as u32, SIDE as u32);
    phash::hashes(&g.into_raw().try_into().unwrap())
}
fn fine(a: &[u64; 4], b: &[u64; 4]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum()
}
fn d(a: &Hashes, b: &Hashes) -> Distances {
    let o = ((a.coarse[0] ^ b.coarse[0]).count_ones(), fine(&a.fine[0], &b.fine[0]));
    let best = (0..8)
        .map(|k| ((a.coarse[0] ^ b.coarse[k]).count_ones(), fine(&a.fine[0], &b.fine[k])))
        .min()
        .unwrap();
    (o, best)
}
fn jpeg(img: &DynamicImage, q: u8) -> DynamicImage {
    let mut buf = Vec::new();
    img.to_rgb8()
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, q))
        .unwrap();
    image::load_from_memory_with_format(&buf, ImageFormat::Jpeg).unwrap()
}
fn crop(img: &DynamicImage, f: f32) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    let (dx, dy) = ((w as f32 * f) as u32, (h as f32 * f) as u32);
    img.crop_imm(dx, dy, w - 2 * dx, h - 2 * dy)
}

fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    let names = [
        "reduite_q60",
        "nb",
        "lumiere",
        "recadree3",
        "recadree8",
        "filigrane",
        "rot90",
        "miroir",
    ];
    let rows: Vec<(Hashes, Vec<Distances>)> = files
        .par_iter()
        .map(|f| {
            let img = image::open(f).unwrap();
            let h0 = hash(&img);
            let small = img.resize_exact(img.width() / 2, img.height() / 2, imageops::FilterType::Triangle);
            let mut wm = img.to_rgb8();
            let (w, h) = (wm.width(), wm.height());
            for y in h * 90 / 100..h * 96 / 100 {
                for x in w * 75 / 100..w * 97 / 100 {
                    wm.put_pixel(x, y, image::Rgb([255, 255, 255]));
                }
            }
            let variants = [
                jpeg(&small, 60),
                DynamicImage::ImageLuma8(img.to_luma8()),
                DynamicImage::ImageRgba8(imageops::colorops::contrast(
                    &imageops::colorops::brighten(&img, 25),
                    15.0,
                )),
                crop(&img, 0.03),
                crop(&img, 0.08),
                DynamicImage::ImageRgb8(wm),
                img.rotate90(),
                img.fliph(),
            ];
            (h0, variants.iter().map(|v| d(&h0, &hash(v))).collect())
        })
        .collect();

    println!(
        "variante      | grossiere (orig) max/p95 | fine (orig) max/p95 | grossiere (8 orient.) max | fine (8) max | retrouvees a 10/40 (8 orient.)"
    );
    for (k, n) in names.iter().enumerate() {
        let mut co: Vec<u32> = rows.iter().map(|r| r.1[k].0.0).collect();
        let mut fo: Vec<u32> = rows.iter().map(|r| r.1[k].0.1).collect();
        let c8 = rows.iter().map(|r| r.1[k].1.0).max().unwrap();
        let f8 = rows.iter().map(|r| r.1[k].1.1).max().unwrap();
        let ok = rows.iter().filter(|r| r.1[k].1.0 <= 10 && r.1[k].1.1 <= 40).count();
        co.sort();
        fo.sort();
        let p95 = |v: &Vec<u32>| v[v.len() * 95 / 100];
        println!(
            "{n:13} | {:3} / {:3} | {:3} / {:3} | {c8:3} | {f8:3} | {ok}/{}",
            co.last().unwrap(),
            p95(&co),
            fo.last().unwrap(),
            p95(&fo),
            rows.len()
        );
    }

    // paires de photos differentes
    let hs: Vec<Hashes> = rows.iter().map(|r| r.0).collect();
    let mut pairs = Vec::new();
    for a in 0..hs.len() {
        for b in a + 1..hs.len() {
            pairs.push((a, b, d(&hs[a], &hs[b])));
        }
    }
    println!("\n{} paires de photos differentes", pairs.len());
    for max in [4u32, 10, 14] {
        let c0 = pairs.iter().filter(|p| p.2.0.0 <= max).count();
        let f0 = pairs.iter().filter(|p| p.2.0.0 <= max && p.2.0.1 <= 4 * max).count();
        let c8 = pairs.iter().filter(|p| p.2.1.0 <= max).count();
        let f8 = pairs
            .iter()
            .filter(|p| {
                (0..8).any(|k| {
                    (hs[p.0].coarse[0] ^ hs[p.1].coarse[k]).count_ones() <= max
                        && fine(&hs[p.0].fine[0], &hs[p.1].fine[k]) <= 4 * max
                })
            })
            .count();
        println!(
            "seuil {max:2} : grossiere seule {c0}, grossiere + fine {f0} | huit orientations : grossiere seule {c8}, grossiere + fine {f8}"
        );
    }
    let mut close: Vec<_> = pairs.iter().map(|p| (p.2.1.0, p.2.1.1, p.0, p.1)).collect();
    close.sort();
    println!("\npaires les plus proches (grossiere, fine, photos) :");
    for c in close.iter().take(8) {
        println!(
            "  {:2} {:3}  {} {}",
            c.0,
            c.1,
            files[c.2].file_name().unwrap().to_string_lossy(),
            files[c.3].file_name().unwrap().to_string_lossy()
        );
    }
}
