//! Mesure du moteur. Genere des photos de synthese si le dossier est vide, puis
//! chronometre le decodage seul, l'analyse complete et la comparaison des paires.
//!
//! cargo run --release -p jumelles-core --example mesure -- <dossier> [nombre] [largeur] [hauteur]

use jumelles_core::{DEFAULT_MAX_DISTANCE, Progress, analyse, decode, phash};
use rayon::prelude::*;
use std::path::PathBuf;
use std::time::Instant;

fn photo(w: u32, h: u32, graine: u32) -> image::RgbImage {
    let mut s = graine.wrapping_mul(2_654_435_761);
    let k = (graine % 97) as f32 / 10.0;
    image::RgbImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut v = 120.0 + 60.0 * (fx * (4.0 + k) + k).sin() * (fy * (7.0 - k / 2.0) + k).cos();
        if (fx - 0.2 - 0.06 * k).powi(2) + (fy - 0.7 + 0.05 * k).powi(2) < 0.05 {
            v += 50.0;
        }
        let v = (v + (s >> 27) as f32).clamp(0.0, 255.0) as u8;
        image::Rgb([v, (v as f32 * 0.8) as u8, 255 - v])
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(args.get(1).expect("dossier"));
    let arg = |i: usize, defaut: u32| args.get(i).map_or(defaut, |s| s.parse().expect("nombre"));
    let (n, w, h) = (arg(2, 500), arg(3, 4000), arg(4, 3000));
    let coeurs = rayon::current_num_threads();

    std::fs::create_dir_all(&dir).unwrap();
    if std::fs::read_dir(&dir).unwrap().next().is_none() {
        let t = Instant::now();
        (0..n)
            .into_par_iter()
            .for_each(|i| photo(w, h, i).save(dir.join(format!("{i:05}.jpg"))).unwrap());
        println!(
            "generation : {n} photos de {w} x {h} en {:.1} s",
            t.elapsed().as_secs_f64()
        );
    }

    let files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    let octets: u64 = files.iter().map(|f| f.metadata().unwrap().len()).sum();
    let t = Instant::now();
    let decodees = files.par_iter().filter(|f| decode::small(f).is_ok()).count();
    let s = t.elapsed().as_secs_f64();
    println!(
        "decodage seul : {decodees} sur {} ({:.0} Mo) en {s:.2} s sur {coeurs} coeurs, {:.1} ms par photo et par coeur",
        files.len(),
        octets as f64 / 1_048_576.0,
        s * 1000.0 * coeurs as f64 / files.len() as f64
    );

    let t = Instant::now();
    let a = analyse(std::slice::from_ref(&dir), DEFAULT_MAX_DISTANCE, &Progress::default());
    println!(
        "analyse complete : {} photos, {} groupes en {:.2} s",
        a.photos.len(),
        a.groups.len(),
        t.elapsed().as_secs_f64()
    );

    // Comparaison de toutes les paires sur des empreintes aleatoires
    let m = 100_000;
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut alea = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let hashes: Vec<phash::Hashes> = (0..m)
        .map(|_| phash::Hashes {
            coarse: std::array::from_fn(|_| alea()),
            fine: std::array::from_fn(|_| std::array::from_fn(|_| alea())),
        })
        .collect();
    let t = Instant::now();
    let proches: usize = phash::close_pairs(&hashes, DEFAULT_MAX_DISTANCE)
        .iter()
        .map(Vec::len)
        .sum();
    println!(
        "paires : {m} empreintes aleatoires en {:.2} s, {proches} paires proches",
        t.elapsed().as_secs_f64()
    );
}
