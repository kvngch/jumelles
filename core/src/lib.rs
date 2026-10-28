//! Moteur de Jumelles : parcours des dossiers, empreintes, regroupement.

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
    "jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif",
    "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "raf", "pef", "srw",
];

/// Octets lus pour la premiere empreinte d'un fichier
const HEAD: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct Photo {
    pub path: PathBuf,
    pub size: u64,
    /// Secondes depuis 1970, 0 si le systeme ne la donne pas
    pub modified: i64,
}

/// Compteurs que l'interface lit pendant l'analyse
#[derive(Debug, Default)]
pub struct Progress {
    pub found: AtomicUsize,
    pub read: AtomicUsize,
}

#[derive(Debug, Default)]
pub struct Analysis {
    pub photos: Vec<Photo>,
    /// Fichiers identiques octet pour octet, en indices dans `photos`, du plus lourd au plus leger
    pub groups: Vec<Vec<usize>>,
    /// Dossiers et fichiers illisibles, ignores
    pub unreadable: usize,
}

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

pub fn analyse(roots: &[PathBuf], progress: &Progress) -> Analysis {
    let (photos, unreadable) = walk(roots, progress);
    let (groups, failed) = identical(&photos, progress);
    Analysis { photos, groups, unreadable: unreadable + failed }
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
    for root in roots.iter().filter(|r| !roots.iter().any(|o| o != *r && r.starts_with(o))) {
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
                .map_or(0, |d| d.as_secs() as i64);
            photos.push(Photo { path: entry.into_path(), size: meta.len(), modified });
            progress.found.fetch_add(1, Ordering::Relaxed);
        }
    }
    (photos, unreadable)
}

/// Regroupe les fichiers identiques : par taille, puis par empreinte des HEAD premiers
/// octets, puis par empreinte complete. Un fichier atteint par deux chemins (dossiers
/// imbriques, jonction, lien dur) ne compte qu'une fois, sous le premier chemin.
fn identical(photos: &[Photo], progress: &Progress) -> (Vec<Vec<usize>>, usize) {
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, p) in photos.iter().enumerate().filter(|(_, p)| p.size > 0) {
        by_size.entry(p.size).or_default().push(i);
    }
    let mut candidates: Vec<usize> = by_size.into_values().filter(|g| g.len() > 1).flatten().collect();
    candidates.sort_unstable();

    let heads: Vec<_> = candidates
        .par_iter()
        .map(|&i| {
            let h = head(&photos[i].path);
            progress.read.fetch_add(1, Ordering::Relaxed);
            (i, h)
        })
        .collect();

    let mut failed = 0;
    let mut seen = HashSet::new();
    let mut by_head: HashMap<(u64, u128), Vec<usize>> = HashMap::new();
    for (i, h) in heads {
        match h {
            Ok((key, hash)) if seen.insert(key) => by_head.entry((photos[i].size, hash)).or_default().push(i),
            Ok(_) => {}
            Err(_) => failed += 1,
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
            Err(_) => failed += 1,
        }
    }
    groups.extend(by_full.into_values().filter(|g| g.len() > 1));

    for g in &mut groups {
        g.sort_unstable();
    }
    groups.sort_unstable_by_key(|g| (std::cmp::Reverse(photos[g[0]].size), g[0]));
    (groups, failed)
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
        let nom = |i: &usize| a.photos[*i].path.file_name().unwrap().to_string_lossy().into_owned();
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
        let a = analyse(std::slice::from_ref(&d), &progress);
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
        let a = analyse(&[d.join("sous"), d.clone(), d.clone()], &Progress::default());
        assert_eq!(a.groups.len(), 1);
        assert_eq!(a.groups[0].len(), 2);
        assert_eq!(noms(&a)[0][0], "x.jpg");
        fs::remove_dir_all(d).unwrap();
    }
}
