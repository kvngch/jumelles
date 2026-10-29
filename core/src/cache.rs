//! Cache des empreintes : une photo deja decodee, de meme taille et de meme date de
//! modification, n'est pas decodee une seconde fois.

use crate::Print;
use crate::phash::Hashes;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// En-tete du fichier. La version change a chaque evolution des empreintes : un cache
/// d'une autre version est ignore.
const MAGIC: &[u8; 8] = b"JUMELLES";
const VERSION: u32 = 2;

#[derive(Debug, Clone, Copy)]
struct Entry {
    size: u64,
    modified: i64,
    print: Print,
}

#[derive(Debug, Default)]
pub struct Cache {
    entries: HashMap<PathBuf, Entry>,
}

impl Cache {
    /// Cache lu depuis `path`, vide si le fichier manque, est abime ou vient d'une autre version
    pub fn load(path: &Path) -> Cache {
        File::open(path)
            .and_then(|f| read(&mut BufReader::new(f)))
            .unwrap_or_default()
    }

    /// Ecrit dans un fichier voisin puis le renomme : un cache n'est jamais a moitie ecrit
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let tmp = path.with_extension("tmp");
        let mut w = BufWriter::new(File::create(&tmp)?);
        w.write_all(MAGIC)?;
        w.write_all(&VERSION.to_le_bytes())?;
        for (p, e) in &self.entries {
            let Some(p) = p.to_str() else { continue };
            w.write_all(&(p.len() as u32).to_le_bytes())?;
            w.write_all(p.as_bytes())?;
            w.write_all(&e.size.to_le_bytes())?;
            w.write_all(&e.modified.to_le_bytes())?;
            w.write_all(&e.print.width.to_le_bytes())?;
            w.write_all(&e.print.height.to_le_bytes())?;
            for x in e.print.hashes.coarse.iter().chain(e.print.hashes.fine.iter().flatten()) {
                w.write_all(&x.to_le_bytes())?;
            }
        }
        w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        std::fs::rename(tmp, path)
    }

    pub(crate) fn get(&self, path: &Path, size: u64, modified: i64) -> Option<Print> {
        self.entries
            .get(path)
            .filter(|e| e.size == size && e.modified == modified)
            .map(|e| e.print)
    }

    pub(crate) fn put(&mut self, path: PathBuf, size: u64, modified: i64, print: Print) {
        self.entries.insert(path, Entry { size, modified, print });
    }

    /// Oublie les photos disparues des dossiers analyses, garde celles des autres dossiers
    pub(crate) fn prune(&mut self, roots: &[PathBuf], seen: &HashSet<&Path>) {
        self.entries
            .retain(|p, _| seen.contains(p.as_path()) || !roots.iter().any(|r| p.starts_with(r)));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn read(r: &mut impl Read) -> io::Result<Cache> {
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC || u32::from_le_bytes(array(r)?) != VERSION {
        return Ok(Cache::default());
    }
    let mut entries = HashMap::new();
    loop {
        let mut len = [0u8; 4];
        match r.read_exact(&mut len) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(Cache { entries }),
            Err(e) => return Err(e),
        }
        let mut p = vec![0u8; u32::from_le_bytes(len) as usize];
        r.read_exact(&mut p)?;
        let p = String::from_utf8(p).map_err(io::Error::other)?;
        let size = u64::from_le_bytes(array(r)?);
        let modified = i64::from_le_bytes(array(r)?);
        let width = u32::from_le_bytes(array(r)?);
        let height = u32::from_le_bytes(array(r)?);
        let mut word = || array(r).map(u64::from_le_bytes);
        let mut coarse = [0u64; 8];
        for x in &mut coarse {
            *x = word()?;
        }
        let mut fine = [[0u64; 4]; 8];
        for x in fine.iter_mut().flatten() {
            *x = word()?;
        }
        let print = Print {
            width,
            height,
            hashes: Hashes { coarse, fine },
        };
        entries.insert(PathBuf::from(p), Entry { size, modified, print });
    }
}

fn array<const N: usize>(r: &mut impl Read) -> io::Result<[u8; N]> {
    let mut b = [0u8; N];
    r.read_exact(&mut b)?;
    Ok(b)
}
