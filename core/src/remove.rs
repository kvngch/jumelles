//! Suppression des photos marquees : corbeille de Windows sur les disques qui en ont une,
//! deplacement dans un dossier choisi ailleurs.

use crate::Analysis;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// La photo n'appartient a aucun groupe
    Alone(usize),
    /// Toutes les photos du groupe seraient supprimees
    WholeGroup(usize),
}

/// Fichiers a supprimer, et ceux ecartes parce qu'ils ont change depuis l'analyse
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub remove: Vec<usize>,
    pub changed: Vec<usize>,
}

/// Verifie une demande avant de toucher au disque : chaque photo appartient a un groupe,
/// aucun groupe ne perd toutes ses photos, et un fichier dont la taille ou la date de
/// modification a change depuis l'analyse est ecarte.
pub fn plan(a: &Analysis, photos: &[usize]) -> Result<Plan, Refusal> {
    let group_of: HashMap<usize, usize> = a
        .groups
        .iter()
        .enumerate()
        .flat_map(|(g, members)| members.iter().map(move |m| (m.photo, g)))
        .collect();
    let mut asked: HashMap<usize, usize> = HashMap::new();
    for &p in photos {
        let g = *group_of.get(&p).ok_or(Refusal::Alone(p))?;
        *asked.entry(g).or_default() += 1;
    }
    let mut dedup = photos.to_vec();
    dedup.sort_unstable();
    dedup.dedup();
    if let Some((&g, _)) = asked.iter().find(|&(&g, _)| {
        let wanted = dedup.iter().filter(|p| group_of[p] == g).count();
        wanted >= a.groups[g].len()
    }) {
        return Err(Refusal::WholeGroup(g));
    }
    let (remove, changed) = dedup.into_iter().partition(|&p| unchanged(&a.photos[p]));
    Ok(Plan { remove, changed })
}

fn unchanged(p: &crate::Photo) -> bool {
    std::fs::metadata(&p.path).is_ok_and(|m| {
        let modified = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis());
        m.len() == p.size && modified as i64 == p.modified
    })
}

/// Deplace les fichiers dans `dest`, en ajoutant (2), (3)... au nom deja pris. Rend le
/// nouveau chemin de chaque fichier, ou son erreur.
pub fn move_into(paths: &[PathBuf], dest: &Path) -> Vec<io::Result<PathBuf>> {
    paths
        .iter()
        .map(|p| {
            let target = free_name(dest, p.file_name().ok_or_else(|| io::Error::other("chemin sans nom"))?);
            // un autre volume refuse le renommage : copie puis suppression de l'original
            if std::fs::rename(p, &target).is_err() {
                std::fs::copy(p, &target)?;
                std::fs::remove_file(p)?;
            }
            Ok(target)
        })
        .collect()
}

fn free_name(dest: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let first = dest.join(name);
    if !first.exists() {
        return first;
    }
    let stem = Path::new(name).file_stem().unwrap_or(name).to_string_lossy();
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| dest.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

#[cfg(windows)]
pub use windows_bin::{has_recycle_bin, recycle};

#[cfg(windows)]
mod windows_bin {
    use std::io;
    use std::path::{Path, PathBuf};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_WANTNUKEWARNING, FOFX_RECYCLEONDELETE, FileOperation, IFileOperation,
        IShellItem, SHCreateItemFromParsingName,
    };
    use windows::core::HSTRING;

    /// GetDriveTypeW : 3 pour un disque fixe. Les lecteurs reseau et les supports
    /// amovibles n'ont pas de corbeille.
    const DRIVE_FIXED: u32 = 3;

    pub fn has_recycle_bin(path: &Path) -> bool {
        let mut root = [0u16; 261];
        unsafe {
            if GetVolumePathNameW(&HSTRING::from(path), &mut root).is_err() {
                return false;
            }
            GetDriveTypeW(windows::core::PCWSTR(root.as_ptr())) == DRIVE_FIXED
        }
    }

    /// Envoie les fichiers a la corbeille par l'operation de l'Explorateur, dans un fil COM
    /// a appartement unique comme elle l'exige. Windows previent avant de supprimer
    /// definitivement un fichier qu'il ne peut pas mettre a la corbeille (corbeille
    /// desactivee ou trop petite). Rend les fichiers qui existent encore : refus de
    /// l'utilisateur, fichier verrouille ou erreur.
    pub fn recycle(paths: &[PathBuf], owner: Option<isize>) -> io::Result<Vec<PathBuf>> {
        let paths = paths.to_vec();
        std::thread::spawn(move || -> io::Result<Vec<PathBuf>> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()?;
                let result = (|| -> windows::core::Result<()> {
                    let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
                    op.SetOperationFlags(
                        FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOFX_RECYCLEONDELETE,
                    )?;
                    if let Some(h) = owner {
                        op.SetOwnerWindow(HWND(h as _))?;
                    }
                    for p in &paths {
                        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(p.as_path()), None)?;
                        op.DeleteItem(&item, None)?;
                    }
                    op.PerformOperations()
                })();
                CoUninitialize();
                // une operation abandonnee en cours de route laisse des fichiers en place
                let _ = result;
                Ok(paths.into_iter().filter(|p| p.exists()).collect())
            }
        })
        .join()
        .map_err(|_| io::Error::other("fil de suppression interrompu"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cache, DEFAULT_MAX_DISTANCE, Progress, analyse};
    use std::fs;

    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jumelles-suppr-{}-{nom}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn plan_protege_les_groupes() {
        let d = dossier("plan");
        for n in ["a.jpg", "b.jpg", "c.jpg"] {
            fs::write(d.join(n), b"meme contenu").unwrap();
        }
        fs::write(d.join("seule.jpg"), b"autre contenu").unwrap();
        let a = analyse(
            std::slice::from_ref(&d),
            DEFAULT_MAX_DISTANCE,
            &Progress::default(),
            &mut Cache::default(),
        );
        let idx = |n: &str| a.photos.iter().position(|p| p.path.ends_with(n)).unwrap();
        let (pa, pb, pc) = (idx("a.jpg"), idx("b.jpg"), idx("c.jpg"));

        assert_eq!(
            plan(&a, &[pb, pc]),
            Ok(Plan {
                remove: vec![pb, pc],
                changed: vec![]
            })
        );
        assert_eq!(plan(&a, &[pa, pb, pc]), Err(Refusal::WholeGroup(0)));
        assert_eq!(plan(&a, &[pa, pb, pc, pc]), Err(Refusal::WholeGroup(0)));
        assert_eq!(plan(&a, &[idx("seule.jpg")]), Err(Refusal::Alone(idx("seule.jpg"))));

        // un fichier modifie depuis l'analyse est ecarte
        fs::write(d.join("c.jpg"), b"meme contenu, modifie ensuite").unwrap();
        assert_eq!(
            plan(&a, &[pb, pc]),
            Ok(Plan {
                remove: vec![pb],
                changed: vec![pc]
            })
        );
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn deplacement_sans_ecraser() {
        let d = dossier("deplacement");
        let dest = d.join("doublons");
        fs::create_dir_all(dest.join("x")).unwrap();
        fs::write(d.join("IMG_1.jpg"), b"1").unwrap();
        fs::write(dest.join("IMG_1.jpg"), b"deja la").unwrap();
        fs::write(d.join("IMG_2.jpg"), b"2").unwrap();
        let r = move_into(&[d.join("IMG_1.jpg"), d.join("IMG_2.jpg")], &dest);
        assert_eq!(r[0].as_ref().unwrap(), &dest.join("IMG_1 (2).jpg"));
        assert_eq!(r[1].as_ref().unwrap(), &dest.join("IMG_2.jpg"));
        assert_eq!(fs::read(dest.join("IMG_1.jpg")).unwrap(), b"deja la");
        assert_eq!(fs::read(dest.join("IMG_1 (2).jpg")).unwrap(), b"1");
        assert!(!d.join("IMG_1.jpg").exists() && !d.join("IMG_2.jpg").exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn corbeille() {
        let d = dossier("corbeille");
        assert!(
            has_recycle_bin(&d),
            "le dossier temporaire du runner est sur un disque fixe"
        );
        fs::write(d.join("a.jpg"), b"a").unwrap();
        fs::write(d.join("b.jpg"), b"b").unwrap();
        let reste = recycle(&[d.join("a.jpg"), d.join("b.jpg")], None).unwrap();
        assert!(reste.is_empty(), "{reste:?}");
        assert!(!d.join("a.jpg").exists() && !d.join("b.jpg").exists());
        fs::remove_dir_all(d).unwrap();
    }
}
