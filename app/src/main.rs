// Pas de console derriere la fenetre en release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use jumelles_core::{Analysis, Cache, Format, Progress, remove};
use serde::Serialize;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::http::{Request, Response};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

/// Derniere analyse. L'interface designe les photos par leur indice, les vignettes et la
/// suppression retrouvent ici leur chemin : aucun chemin ne vient de l'interface. Le
/// numero d'analyse entre dans l'adresse des vignettes, pour qu'une vignette gardee par
/// le navigateur ne serve pas une photo d'une autre analyse.
#[derive(Default)]
struct Derniere {
    analyse: Mutex<Option<Analysis>>,
    numero: AtomicU64,
}

fn verrou(app: &AppHandle) -> MutexGuard<'_, Option<Analysis>> {
    app.state::<Derniere>()
        .inner()
        .analyse
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[derive(Serialize)]
struct Fichier {
    id: usize,
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
    numero: u64,
    photos: usize,
    groupes: Vec<Vec<Fichier>>,
    illisibles: usize,
    non_decodees: NonDecodees,
    duree_ms: u64,
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
        let debut = Instant::now();
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
                id: m.photo,
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
        let derniere = app.state::<Derniere>();
        let numero = derniere.numero.fetch_add(1, Ordering::SeqCst) + 1;
        let resultat = Resultat {
            numero,
            photos: analyse.photos.len(),
            groupes: analyse.groups.iter().map(|g| g.iter().map(fichier).collect()).collect(),
            illisibles: analyse.unreadable,
            non_decodees,
            duree_ms: debut.elapsed().as_millis() as u64,
        };
        *verrou(&app) = Some(analyse);
        resultat
    })
    .await
    .map_err(|e| e.to_string())
}

fn chemin(app: &AppHandle, id: usize) -> Option<PathBuf> {
    verrou(app).as_ref()?.photos.get(id).map(|p| p.path.clone())
}

/// vignette://localhost/<numero>/<indice>?t=<cote>, servie sous Windows a l'adresse
/// http://vignette.localhost/<numero>/<indice>?t=<cote>
fn vignette(app: &AppHandle, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let mut parts = request.uri().path().trim_start_matches('/').split('/');
    let numero: Option<u64> = parts.next().and_then(|n| n.parse().ok());
    let id: Option<usize> = parts.next().and_then(|i| i.parse().ok());
    let cote: u32 = request
        .uri()
        .query()
        .and_then(|q| q.strip_prefix("t="))
        .and_then(|t| t.parse().ok())
        .unwrap_or(400)
        .clamp(16, 2400);
    let actuel = numero == Some(app.state::<Derniere>().numero.load(Ordering::SeqCst));
    let photo = id.filter(|_| actuel).and_then(|i| chemin(app, i));
    match photo.map(|p| jumelles_core::decode::thumbnail(&p, cote)) {
        Some(Ok(jpeg)) => Response::builder()
            .header("Content-Type", "image/jpeg")
            .header("Cache-Control", "max-age=31536000, immutable")
            .body(jpeg),
        _ => Response::builder().status(404).body(Vec::new()),
    }
    .unwrap_or_default()
}

/// Ouvre la photo dans la visionneuse de Windows
#[tauri::command]
fn ouvrir(app: AppHandle, id: usize) -> Result<(), String> {
    let p = chemin(&app, id).ok_or("photo inconnue")?;
    tauri_plugin_opener::open_path(p, None::<&str>).map_err(|e| e.to_string())
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct Bilan {
    /// Photos parties, a la corbeille ou deplacees
    parties: Vec<usize>,
    /// Photos encore en place : fichier ouvert ailleurs, refus de l'utilisateur, erreur
    restees: Vec<usize>,
    /// Photos sur un lecteur sans corbeille (reseau, support amovible), a deplacer
    sans_corbeille: Vec<usize>,
    /// Photos modifiees depuis l'analyse, laissees en place
    modifiees: Vec<usize>,
}

/// Photos a traiter avec leur chemin, et photos modifiees depuis l'analyse
type Preparation = (Vec<(usize, PathBuf)>, Vec<usize>);

/// Plan verifie par le moteur, avec le chemin de chaque photo a traiter
fn preparer(app: &AppHandle, ids: &[usize]) -> Result<Preparation, String> {
    let garde = verrou(app);
    let a = garde.as_ref().ok_or("aucune-analyse")?;
    let plan = remove::plan(a, ids).map_err(|r| match r {
        remove::Refusal::Alone(_) => "refus-photo".to_string(),
        remove::Refusal::WholeGroup(_) => "refus-groupe".to_string(),
    })?;
    Ok((
        plan.remove.iter().map(|&i| (i, a.photos[i].path.clone())).collect(),
        plan.changed,
    ))
}

#[cfg(windows)]
fn fenetre(w: &WebviewWindow) -> Option<isize> {
    w.hwnd().ok().map(|h| h.0 as isize)
}

#[cfg(not(windows))]
fn fenetre(_: &WebviewWindow) -> Option<isize> {
    None
}

/// Envoie les photos a la corbeille. Celles d'un lecteur sans corbeille sont rendues a
/// l'interface, qui propose de les deplacer.
#[tauri::command]
async fn supprimer(app: AppHandle, window: WebviewWindow, ids: Vec<usize>) -> Result<Bilan, String> {
    let owner = fenetre(&window);
    tauri::async_runtime::spawn_blocking(move || {
        let (photos, modifiees) = preparer(&app, &ids)?;
        let (corbeille, sans): (Vec<_>, Vec<_>) = photos.into_iter().partition(|(_, p)| remove::has_recycle_bin(p));
        let chemins: Vec<PathBuf> = corbeille.iter().map(|(_, p)| p.clone()).collect();
        let encore: HashSet<PathBuf> = if chemins.is_empty() {
            HashSet::new()
        } else {
            remove::recycle(&chemins, owner)
                .map_err(|e| e.to_string())?
                .into_iter()
                .collect()
        };
        let (restees, parties): (Vec<_>, Vec<_>) = corbeille.into_iter().partition(|(_, p)| encore.contains(p));
        let bilan = Bilan {
            parties: parties.into_iter().map(|(i, _)| i).collect(),
            restees: restees.into_iter().map(|(i, _)| i).collect(),
            sans_corbeille: sans.into_iter().map(|(i, _)| i).collect(),
            modifiees,
        };
        if let Some(a) = verrou(&app).as_mut() {
            remove::forget(a, &bilan.parties);
        }
        Ok(bilan)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Deplace les photos dans un dossier, sans ecraser un fichier deja present
#[tauri::command]
async fn deplacer(app: AppHandle, ids: Vec<usize>, dossier: PathBuf) -> Result<Bilan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (photos, modifiees) = preparer(&app, &ids)?;
        let chemins: Vec<PathBuf> = photos.iter().map(|(_, p)| p.clone()).collect();
        let mut bilan = Bilan {
            modifiees,
            ..Default::default()
        };
        for ((i, _), r) in photos.iter().zip(remove::move_into(&chemins, &dossier)) {
            if r.is_ok() {
                bilan.parties.push(*i)
            } else {
                bilan.restees.push(*i)
            }
        }
        if let Some(a) = verrou(&app).as_mut() {
            remove::forget(a, &bilan.parties);
        }
        Ok(bilan)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    tauri::Builder::default()
        .manage(Derniere::default())
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("vignette", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            // fils de calcul de rayon : leur fabrique WIC sert d'une vignette a l'autre
            rayon::spawn(move || responder.respond(vignette(&app, &request)));
        })
        .invoke_handler(tauri::generate_handler![analyser, ouvrir, supprimer, deplacer])
        .run(tauri::generate_context!())
        .expect("lancement de Jumelles");
}
