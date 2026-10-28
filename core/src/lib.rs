//! Moteur de Jumelles : parcours des dossiers, empreintes, regroupement.

pub mod cache;
pub mod decode;
pub mod phash;

pub use cache::Cache;

use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;
use xxhash_rust::xxh3::{Xxh3, xxh3_128};

/// Extensions retenues par le parcours, comparees sans tenir compte de la casse.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif", "cr2", "cr3", "nef",
    "arw", "dng", "orf", "rw2", "raf", "pef", "srw",
];

/// Octets lus pour la premiere empreinte d'un fichier
const HEAD: usize = 64 * 1024;

/// Distance maximale par defaut entre deux photos proches, en bits d'empreinte sur 64
pub const DEFAULT_MAX_DISTANCE: u32 = 10;

#[derive(Debug, Clone)]
pub struct Photo {
    pub path: PathBuf,
    pub size: u64,
    /// Millisecondes depuis 1970, 0 si le systeme ne la donne pas
    pub modified: i64,
    /// Dimensions et empreintes, absentes si la photo n'a pas ete decodee
    pub print: Option<Print>,
}

#[derive(Debug, Clone, Copy)]
pub struct Print {
    pub width: u32,
    pub height: u32,
    pub hashes: phash::Hashes,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Member {
    /// Indice dans `Analysis::photos`
    pub photo: usize,
    /// Identique octet pour octet a la reference du groupe
    pub identical: bool,
    /// Bits d'empreinte fine differents de la reference du groupe, sur 256
    pub distance: u32,
}

/// Compteurs que l'interface lit pendant l'analyse
#[derive(Debug, Default)]
pub struct Progress {
    pub found: AtomicUsize,
    pub to_read: AtomicUsize,
    pub read: AtomicUsize,
    pub to_decode: AtomicUsize,
    pub decoded: AtomicUsize,
}

#[derive(Debug, Default)]
pub struct Analysis {
    pub photos: Vec<Photo>,
    /// La reference en premier : plus grande definition, puis plus ancienne. Les groupes
    /// qui liberent le plus de place viennent d'abord.
    pub groups: Vec<Vec<Member>>,
    /// Dossiers et fichiers illisibles, ignores
    pub unreadable: usize,
    /// Photos lues mais pas decodees
    pub undecodable: usize,
    /// Photos decodees pendant cette analyse, les autres venant du cache
    pub decoded: usize,
}

/// Copies exactes, chemins a ecarter (second chemin vers un meme fichier, fichier
/// illisible) et nombre de fichiers illisibles
struct Exact {
    groups: Vec<Vec<usize>>,
    dropped: Vec<bool>,
    failed: usize,
}

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

pub fn analyse(roots: &[PathBuf], max_distance: u32, progress: &Progress, cache: &mut Cache) -> Analysis {
    let (mut photos, unreadable) = walk(roots, progress);
    let exact = identical(&photos, progress);

    // Une photo par groupe de copies exactes est decodee, les autres en heritent
    let mut skip = exact.dropped.clone();
    for g in &exact.groups {
        for &i in &g[1..] {
            skip[i] = true;
        }
    }
    let reps: Vec<usize> = (0..photos.len()).filter(|&i| !skip[i]).collect();
    progress.to_decode.store(reps.len(), Ordering::Relaxed);
    let known: &Cache = cache;
    let prints: Vec<(Option<Print>, bool)> = reps
        .par_iter()
        .map(|&i| {
            let p = &photos[i];
            let print = match known.get(&p.path, p.size, p.modified) {
                Some(print) => (Some(print), false),
                None => {
                    let print = decode::small(&p.path).ok().map(|s| Print {
                        width: s.width,
                        height: s.height,
                        hashes: phash::hashes(&s.px),
                    });
                    (print, true)
                }
            };
            progress.decoded.fetch_add(1, Ordering::Relaxed);
            print
        })
        .collect();
    let undecodable = prints.iter().filter(|p| p.0.is_none()).count();
    let decoded = prints.iter().filter(|p| p.1).count();
    for (&i, (print, _)) in reps.iter().zip(prints) {
        photos[i].print = print;
    }
    for g in &exact.groups {
        let print = photos[g[0]].print;
        for &i in &g[1..] {
            photos[i].print = print;
        }
    }
    for p in photos.iter().filter(|p| p.print.is_some()) {
        cache.put(p.path.clone(), p.size, p.modified, p.print.unwrap());
    }
    cache.prune(roots, &photos.iter().map(|p| p.path.as_path()).collect());

    let groups = group(&photos, &reps, &exact.groups, max_distance);
    Analysis {
        photos,
        groups,
        unreadable: unreadable + exact.failed,
        undecodable,
        decoded,
    }
}

/// Rassemble les photos proches autour de la meilleure, plus grande definition puis plus
/// ancienne. Une photo ne rejoint un groupe que si elle est proche de sa reference, jamais
/// par ricochet d'une photo proche a une autre : la reference est celle que l'ecran de tri
/// propose de garder.
fn group(photos: &[Photo], reps: &[usize], exact: &[Vec<usize>], max: u32) -> Vec<Vec<Member>> {
    let mut copies: HashMap<usize, &[usize]> = exact.iter().map(|g| (g[0], &g[1..])).collect();
    let pixels = |i: usize| photos[i].print.map_or(0, |p| p.width as u64 * p.height as u64);
    let mut order: Vec<usize> = reps.iter().copied().filter(|&i| photos[i].print.is_some()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(pixels(i)), photos[i].modified, i));
    let hashes: Vec<phash::Hashes> = order
        .iter()
        .filter_map(|&i| photos[i].print.map(|p| p.hashes))
        .collect();

    let mut near = phash::close_pairs(&hashes, max);

    let member = |photo, identical, distance| Member {
        photo,
        identical,
        distance,
    };
    let mut taken = vec![false; order.len()];
    let mut groups = Vec::new();
    for a in 0..order.len() {
        if taken[a] {
            continue;
        }
        let r = order[a];
        let mut members = vec![member(r, true, 0)];
        members.extend(
            copies
                .remove(&r)
                .unwrap_or_default()
                .iter()
                .map(|&c| member(c, true, 0)),
        );
        // la plus ancienne des copies exactes de la reference passe en tete
        members.sort_by_key(|m| (photos[m.photo].modified, m.photo));
        for (b, d) in std::mem::take(&mut near[a]) {
            if !taken[b] {
                taken[b] = true;
                let x = order[b];
                members.push(member(x, false, d));
                members.extend(
                    copies
                        .remove(&x)
                        .unwrap_or_default()
                        .iter()
                        .map(|&c| member(c, false, d)),
                );
            }
        }
        if members.len() > 1 {
            groups.push(members);
        }
    }
    // copies exactes dont la photo n'a pas ete decodee
    for (r, rest) in copies {
        let mut members: Vec<Member> = std::iter::once(r)
            .chain(rest.iter().copied())
            .map(|c| member(c, true, 0))
            .collect();
        members.sort_by_key(|m| (photos[m.photo].modified, m.photo));
        groups.push(members);
    }
    groups.sort_by_key(|g| {
        (
            std::cmp::Reverse(g[1..].iter().map(|m| photos[m.photo].size).sum::<u64>()),
            g[0].photo,
        )
    });
    groups
}

/// Dossiers systeme de Windows, illisibles et sans photos de l'utilisateur
fn is_system_dir(name: &OsStr) -> bool {
    name.eq_ignore_ascii_case("$RECYCLE.BIN") || name.eq_ignore_ascii_case("System Volume Information")
}

/// Parcourt les dossiers et leurs sous-dossiers sans suivre les liens. Un dossier
/// contenu dans un autre de la liste n'est parcouru qu'une fois.
fn walk(roots: &[PathBuf], progress: &Progress) -> (Vec<Photo>, usize) {
    let mut roots = roots.to_vec();
    roots.sort();
    roots.dedup();
    let mut photos = Vec::new();
    let mut unreadable = 0;
    for root in roots
        .iter()
        .filter(|r| !roots.iter().any(|o| o != *r && r.starts_with(o)))
    {
        let entries = WalkDir::new(root)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|e| !(e.file_type().is_dir() && is_system_dir(e.file_name())));
        for entry in entries {
            let Ok(entry) = entry else {
                unreadable += 1;
                continue;
            };
            if !entry.file_type().is_file() || !is_image(entry.path()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                unreadable += 1;
                continue;
            };
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as i64);
            photos.push(Photo {
                path: entry.into_path(),
                size: meta.len(),
                modified,
                print: None,
            });
            progress.found.fetch_add(1, Ordering::Relaxed);
        }
    }
    (photos, unreadable)
}

/// Regroupe les fichiers identiques : par taille, puis par empreinte des HEAD premiers
/// octets, puis par empreinte complete. Un fichier atteint par deux chemins (dossiers
/// imbriques, jonction, lien dur) ne compte qu'une fois, sous le premier chemin.
fn identical(photos: &[Photo], progress: &Progress) -> Exact {
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, p) in photos.iter().enumerate().filter(|(_, p)| p.size > 0) {
        by_size.entry(p.size).or_default().push(i);
    }
    let mut candidates: Vec<usize> = by_size.into_values().filter(|g| g.len() > 1).flatten().collect();
    candidates.sort_unstable();
    progress.to_read.store(candidates.len(), Ordering::Relaxed);

    let heads: Vec<_> = candidates
        .par_iter()
        .map(|&i| {
            let h = head(&photos[i].path);
            progress.read.fetch_add(1, Ordering::Relaxed);
            (i, h)
        })
        .collect();

    let mut failed = 0;
    let mut dropped = vec![false; photos.len()];
    let mut seen = HashSet::new();
    let mut by_head: HashMap<(u64, u128), Vec<usize>> = HashMap::new();
    for (i, h) in heads {
        match h {
            Ok((key, hash)) if seen.insert(key) => by_head.entry((photos[i].size, hash)).or_default().push(i),
            Ok(_) => dropped[i] = true,
            Err(_) => {
                failed += 1;
                dropped[i] = true;
            }
        }
    }

    let mut groups = Vec::new();
    let mut rest = Vec::new();
    for g in by_head.into_values().filter(|g| g.len() > 1) {
        if photos[g[0]].size <= HEAD as u64 {
            groups.push(g);
        } else {
            rest.extend(g);
        }
    }
    let full: Vec<_> = rest.par_iter().map(|&i| (i, full_hash(&photos[i].path))).collect();
    let mut by_full: HashMap<(u64, u128), Vec<usize>> = HashMap::new();
    for (i, h) in full {
        match h {
            Ok(hash) => by_full.entry((photos[i].size, hash)).or_default().push(i),
            Err(_) => {
                failed += 1;
                dropped[i] = true;
            }
        }
    }
    groups.extend(by_full.into_values().filter(|g| g.len() > 1));

    for g in &mut groups {
        g.sort_unstable();
    }
    Exact {
        groups,
        dropped,
        failed,
    }
}

/// Identite du fichier et empreinte de son debut, lues sur une seule ouverture
fn head(path: &Path) -> io::Result<((u64, u64), u128)> {
    let file = File::open(path)?;
    let key = file_key(&file)?;
    let mut buf = Vec::with_capacity(HEAD);
    file.take(HEAD as u64).read_to_end(&mut buf)?;
    Ok((key, xxh3_128(&buf)))
}

fn full_hash(path: &Path) -> io::Result<u128> {
    let mut file = File::open(path)?;
    let mut hasher = Xxh3::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(hasher.digest128());
        }
        hasher.update(&buf[..n]);
    }
}

#[cfg(unix)]
fn file_key(file: &File) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata()?;
    Ok((m.dev(), m.ino()))
}

// Numero de serie du volume et index du fichier, sans la lettre de lecteur : un lecteur
// substitue (subst) ou une jonction menent au meme fichier, qui ne doit jamais
// apparaitre comme sa propre copie.
#[cfg(windows)]
fn file_key(file: &File) -> io::Result<(u64, u64)> {
    let info = winapi_util::file::information(file)?;
    Ok((info.volume_serial_number(), info.file_index()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jumelles-{}-{nom}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn noms(a: &Analysis) -> Vec<Vec<String>> {
        let nom = |m: &Member| {
            a.photos[m.photo]
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        a.groups.iter().map(|g| g.iter().map(nom).collect()).collect()
    }

    #[test]
    fn extensions() {
        assert!(is_image(Path::new("vacances/IMG_0001.JPG")));
        assert!(is_image(Path::new("a.heic")));
        assert!(is_image(Path::new("b.Cr3")));
        assert!(!is_image(Path::new("notes.txt")));
        assert!(!is_image(Path::new("jpg")));
        assert!(!is_image(Path::new("archive.jpg.zip")));
    }

    #[test]
    fn copies_exactes() {
        let d = dossier("copies");
        fs::create_dir_all(d.join("sous")).unwrap();
        fs::create_dir_all(d.join("$RECYCLE.BIN")).unwrap();
        let gros: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        fs::write(d.join("a.jpg"), &gros).unwrap();
        fs::write(d.join("sous/a-copie.JPG"), &gros).unwrap();
        fs::write(d.join("$RECYCLE.BIN/a-jetee.jpg"), &gros).unwrap();
        fs::write(d.join("notes.txt"), &gros).unwrap();
        // meme taille et meme debut que a.jpg, dernier octet different
        let mut fin = gros.clone();
        *fin.last_mut().unwrap() ^= 1;
        fs::write(d.join("b.jpg"), &fin).unwrap();
        // meme taille, contenu different des le debut
        fs::write(d.join("c.png"), vec![7u8; 200_000]).unwrap();
        fs::write(d.join("petit1.gif"), b"GIF89a").unwrap();
        fs::write(d.join("petit2.gif"), b"GIF89a").unwrap();
        fs::write(d.join("vide1.jpg"), b"").unwrap();
        fs::write(d.join("vide2.jpg"), b"").unwrap();

        let progress = Progress::default();
        let a = analyse(
            std::slice::from_ref(&d),
            DEFAULT_MAX_DISTANCE,
            &progress,
            &mut Cache::default(),
        );
        assert_eq!(a.photos.len(), 8);
        assert_eq!(progress.found.load(Ordering::Relaxed), 8);
        assert_eq!(noms(&a), [["a.jpg", "a-copie.JPG"], ["petit1.gif", "petit2.gif"]]);
        assert_eq!(a.unreadable, 0);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn un_fichier_par_deux_chemins() {
        let d = dossier("chemins");
        fs::create_dir_all(d.join("sous")).unwrap();
        fs::write(d.join("sous/x.jpg"), b"photo x").unwrap();
        fs::write(d.join("y.jpg"), b"photo x").unwrap();
        #[cfg(unix)]
        fs::hard_link(d.join("y.jpg"), d.join("y-lien.jpg")).unwrap();

        // dossier imbrique et dossier donne deux fois
        let a = analyse(
            &[d.join("sous"), d.clone(), d.clone()],
            DEFAULT_MAX_DISTANCE,
            &Progress::default(),
            &mut Cache::default(),
        );
        assert_eq!(a.groups.len(), 1);
        assert_eq!(a.groups[0].len(), 2);
        assert_eq!(noms(&a)[0][0], "x.jpg");
        fs::remove_dir_all(d).unwrap();
    }

    /// Photo de synthese au spectre riche : ondulations, tache claire et grain
    fn photo(w: u32, h: u32, graine: u32) -> image::RgbImage {
        let mut s = graine;
        image::RgbImage::from_fn(w, h, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let k = (graine % 5) as f32;
            let mut v = 120.0 + 60.0 * (fx * (4.0 + k) + k).sin() * (fy * (7.0 - k) + k).cos();
            if (fx - 0.2 - 0.12 * k).powi(2) + (fy - 0.7 + 0.1 * k).powi(2) < 0.05 {
                v += 50.0;
            }
            let v = (v + (s >> 26) as f32).clamp(0.0, 255.0) as u8;
            image::Rgb([v, (v as f32 * 0.8) as u8, 255 - v])
        })
    }

    #[test]
    fn photos_proches() {
        let d = dossier("proches");
        let a = photo(640, 480, 1);
        a.save(d.join("a.jpg")).unwrap();
        fs::copy(d.join("a.jpg"), d.join("a-copie.jpg")).unwrap();
        image::imageops::resize(&a, 320, 240, image::imageops::FilterType::Triangle)
            .save(d.join("a-petite.jpg"))
            .unwrap();
        image::DynamicImage::ImageRgb8(a)
            .to_luma8()
            .save(d.join("a-nb.png"))
            .unwrap();
        image::imageops::rotate90(&photo(640, 480, 1))
            .save(d.join("a-tournee.jpg"))
            .unwrap();
        photo(640, 480, 7).save(d.join("b.jpg")).unwrap();

        let r = analyse(
            std::slice::from_ref(&d),
            DEFAULT_MAX_DISTANCE,
            &Progress::default(),
            &mut Cache::default(),
        );
        assert_eq!(r.undecodable, 0);
        assert_eq!(r.groups.len(), 1, "{:?}", noms(&r));
        let g = &r.groups[0];
        let nom = |m: &Member| {
            r.photos[m.photo]
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        let mut tries: Vec<String> = g.iter().map(nom).collect();
        tries.sort();
        assert_eq!(
            tries,
            ["a-copie.jpg", "a-nb.png", "a-petite.jpg", "a-tournee.jpg", "a.jpg"]
        );
        assert!(nom(&g[0]).ends_with(".jpg") && nom(&g[0]).starts_with("a") && nom(&g[0]) != "a-petite.jpg");
        assert!(g[1].identical && nom(&g[1]) != "a-petite.jpg" && nom(&g[1]) != "a-nb.png");
        for m in &g[2..] {
            assert!(
                !m.identical && m.distance <= 4 * DEFAULT_MAX_DISTANCE,
                "{} a {}",
                nom(m),
                m.distance
            );
        }
        // distances relevees pour regler le seuil
        let print = |n: &str| {
            r.photos
                .iter()
                .find(|p| p.path.ends_with(n))
                .unwrap()
                .print
                .unwrap()
                .hashes
        };
        for n in ["a-petite.jpg", "a-nb.png", "a-tournee.jpg", "b.jpg"] {
            let (a, b) = (print("a.jpg"), print(n));
            let fine: u32 = a.fine[0]
                .iter()
                .zip(&b.fine[0])
                .map(|(x, y)| (x ^ y).count_ones())
                .sum();
            eprintln!(
                "a.jpg / {n} : {} sur 64, {fine} sur 256",
                (a.coarse[0] ^ b.coarse[0]).count_ones()
            );
        }
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn cache_des_empreintes() {
        let d = dossier("cache");
        photo(320, 240, 1).save(d.join("a.jpg")).unwrap();
        photo(320, 240, 2).save(d.join("b.jpg")).unwrap();
        let fichier = d.join("jumelles.cache");
        let analyser = |cache: &mut Cache| {
            analyse(
                std::slice::from_ref(&d),
                DEFAULT_MAX_DISTANCE,
                &Progress::default(),
                cache,
            )
        };

        let mut cache = Cache::load(&fichier);
        assert!(cache.is_empty());
        assert_eq!(analyser(&mut cache).decoded, 2);
        cache.save(&fichier).unwrap();

        // deuxieme analyse : rien a decoder
        let mut cache = Cache::load(&fichier);
        assert_eq!(cache.len(), 2);
        let r = analyser(&mut cache);
        assert_eq!(r.decoded, 0);
        assert!(r.photos.iter().all(|p| p.print.is_some()));

        // une photo modifiee est decodee de nouveau, une photo supprimee est oubliee
        photo(400, 300, 3).save(d.join("a.jpg")).unwrap();
        fs::remove_file(d.join("b.jpg")).unwrap();
        let r = analyser(&mut cache);
        assert_eq!(r.decoded, 1);
        assert_eq!(r.photos[0].print.unwrap().width, 400);
        assert_eq!(cache.len(), 1);

        // un cache abime se lit comme un cache vide
        fs::write(&fichier, b"JUMELLES\x01\x00\x00\x00\x05\x00").unwrap();
        assert!(Cache::load(&fichier).is_empty());
        fs::remove_dir_all(d).unwrap();
    }
}
