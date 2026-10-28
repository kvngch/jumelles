// Pas de console derriere la fenetre en release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use jumelles_core::{Cache, Format, Progress};
use serde::Serialize;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Serialize)]
struct Fichier {
    chemin: String,
    taille: u64,
    modifie: i64,
    largeur: u32,
    hauteur: u32,
    identique: bool,
    ecart: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Resultat {
    photos: usize,
    groupes: Vec<Vec<Fichier>>,
    illisibles: usize,
    non_decodees: NonDecodees,
}

/// Photos non decodees par famille de format, pour dire quelle extension installer
#[derive(Serialize, Default)]
struct NonDecodees {
    heif: usize,
    raw: usize,
    autres: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Avancement {
    trouvees: usize,
    a_lire: usize,
    lues: usize,
    a_decoder: usize,
    decodees: usize,
}

/// Fichier du cache : a cote de l'exe si son dossier accepte l'ecriture, sinon dans
/// %LOCALAPPDATA%\Jumelles
fn cache_file() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok().map(|e| e.with_file_name("jumelles.cache"));
    if let Some(p) = beside.filter(|p| OpenOptions::new().create(true).append(true).open(p).is_ok()) {
        return Some(p);
    }
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Jumelles");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("jumelles.cache"))
}

#[tauri::command]
async fn analyser(app: AppHandle, dossiers: Vec<PathBuf>, ecart: u32) -> Result<Resultat, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let progress = Progress::default();
        let fini = AtomicBool::new(false);
        let analyse = std::thread::scope(|s| {
            s.spawn(|| {
                while !fini.load(Ordering::Relaxed) {
                    let _ = app.emit(
                        "avancement",
                        Avancement {
                            trouvees: progress.found.load(Ordering::Relaxed),
                            a_lire: progress.to_read.load(Ordering::Relaxed),
                            lues: progress.read.load(Ordering::Relaxed),
                            a_decoder: progress.to_decode.load(Ordering::Relaxed),
                            decodees: progress.decoded.load(Ordering::Relaxed),
                        },
                    );
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
            let fichier = cache_file();
            let mut cache = fichier.as_deref().map(Cache::load).unwrap_or_default();
            let a = jumelles_core::analyse(&dossiers, ecart, &progress, &mut cache);
            // un cache qui ne s'ecrit pas ne coute qu'un decodage complet a la prochaine analyse
            if let Some(f) = &fichier {
                let _ = cache.save(f);
            }
            fini.store(true, Ordering::Relaxed);
            a
        });
        let fichier = |m: &jumelles_core::Member| {
            let p = &analyse.photos[m.photo];
            let (largeur, hauteur) = p.print.map_or((0, 0), |x| (x.width, x.height));
            Fichier {
                chemin: p.path.to_string_lossy().into_owned(),
                taille: p.size,
                modifie: p.modified,
                largeur,
                hauteur,
                identique: m.identical,
                ecart: m.distance,
            }
        };
        let mut non_decodees = NonDecodees::default();
        for &i in &analyse.undecodable {
            match jumelles_core::format(&analyse.photos[i].path) {
                Some(Format::Heif) => non_decodees.heif += 1,
                Some(Format::Raw) => non_decodees.raw += 1,
                _ => non_decodees.autres += 1,
            }
        }
        Resultat {
            photos: analyse.photos.len(),
            groupes: analyse.groups.iter().map(|g| g.iter().map(fichier).collect()).collect(),
            illisibles: analyse.unreadable,
            non_decodees,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![analyser])
        .run(tauri::generate_context!())
        .expect("lancement de Jumelles");
}
