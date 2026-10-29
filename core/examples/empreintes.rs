//! Distances d'empreinte entre paires de photos, decodees comme pendant l'analyse : bits
//! differents sur 64 (empreinte grossiere) et sur 256 (fine), meilleure des huit
//! orientations. Ecrit aussi les 32 x 32 niveaux de gris de chaque photo en PGM si un
//! dossier est donne.
//!
//! cargo run --release -p jumelles-core --example empreintes -- [--gris <dossier>] <a> <b> [<a> <b>...]

use jumelles_core::{decode, phash};
use std::path::{Path, PathBuf};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let gris = (args.first().map(String::as_str) == Some("--gris")).then(|| {
        args.remove(0);
        PathBuf::from(args.remove(0))
    });
    let fine = |a: &[u64; 4], b: &[u64; 4]| a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum::<u32>();
    let lire = |p: &str| {
        let s = decode::small(Path::new(p)).map_err(|e| format!("{p} : {e}"))?;
        if let Some(d) = &gris {
            let nom = Path::new(p).file_name().unwrap().to_string_lossy();
            let mut pgm = format!("P5\n{} {}\n255\n", phash::SIDE, phash::SIDE).into_bytes();
            pgm.extend_from_slice(&s.px);
            std::fs::write(d.join(format!("{nom}.pgm")), pgm).map_err(|e| e.to_string())?;
        }
        Ok::<_, String>(phash::hashes(&s.px))
    };
    for paire in args.chunks(2) {
        match (lire(&paire[0]), lire(&paire[1])) {
            (Ok(a), Ok(b)) => {
                let (grossiere, fin) = (0..8)
                    .map(|k| ((a.coarse[0] ^ b.coarse[k]).count_ones(), fine(&a.fine[0], &b.fine[k])))
                    .min()
                    .unwrap();
                let proches = phash::near(&a, &b, jumelles_core::DEFAULT_MAX_DISTANCE).is_some();
                println!("{grossiere:>3} {fin:>4} {proches:>5}  {} / {}", paire[0], paire[1]);
            }
            (a, b) => println!("echec : {:?} {:?}", a.err(), b.err()),
        }
    }
}
