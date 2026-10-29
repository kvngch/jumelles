"""Recette de Jumelles sous Windows : l'exe de release pilote de bout en bout.

python recette/recette.py <exe> <jeu> <sortie> [<dossier des extensions>]

L'interface est pilotee par le protocole DevTools de WebView2 (Playwright), les dialogues
de Windows par UI Automation (pywinauto). Chaque verification ecrit une ligne du rapport,
un echec n'arrete pas la recette, et le script sort en erreur s'il y en a eu un. Les
extensions d'image du Microsoft Store, si leur dossier est donne, sont installees pour le
dernier scenario seulement.
"""

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
import traceback
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

import psutil
from PIL import Image, ImageGrab
from playwright.sync_api import sync_playwright
from pywinauto import Desktop, findwindows
import win32api
import win32gui

EXE, JEU, SORTIE = (Path(a).resolve() for a in sys.argv[1:4])
EXTENSIONS = Path(sys.argv[4]).resolve() if len(sys.argv) > 4 else None
ATTENDU = json.loads((JEU / "attendu.json").read_text(encoding="utf-8"))
RAW = {".cr2", ".cr3", ".nef", ".arw", ".dng", ".orf", ".rw2", ".raf", ".pef", ".srw"}
# Variantes dont l'issue n'est pas une regle de Jumelles : rapportees sans verdict
INDICATIVES = {"recadree", "tronquee", "chemin-long", "apercu"}


class Rapport:
    def __init__(self):
        self.lignes = ["# Recette de Jumelles sous Windows", ""]
        self.echecs = []
        SORTIE.mkdir(parents=True, exist_ok=True)

    def section(self, titre):
        self.lignes += ["", f"## {titre}", ""]
        print(f"\n== {titre}", flush=True)

    def ok(self, condition, texte, detail=""):
        marque = "OK" if condition else "ÉCHEC"
        ligne = f"- {marque} : {texte}" + (f" ({detail})" if detail else "")
        self.lignes.append(ligne)
        print(ligne, flush=True)
        if not condition:
            self.echecs.append(texte)
        return condition

    def note(self, texte):
        self.lignes.append(f"- {texte}")
        print(f"- {texte}", flush=True)

    def bloc(self, texte):
        self.lignes += ["", "```", texte.rstrip(), "```", ""]
        print(texte, flush=True)

    def capture(self, page, nom):
        page.screenshot(path=SORTIE / f"{nom}.png")
        self.lignes.append(f"- capture : `{nom}.png`")

    def ecrire(self):
        self.lignes += ["", f"**{len(self.echecs)} échec(s)**"] + [f"- {e}" for e in self.echecs]
        texte = "\n".join(self.lignes) + "\n"
        (SORTIE / "rapport.md").write_text(texte, encoding="utf-8")
        if os.environ.get("GITHUB_STEP_SUMMARY"):
            with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as f:
                f.write(texte)


R = Rapport()


def ps(commande):
    commande = "$ProgressPreference = 'SilentlyContinue'; " + commande
    p = subprocess.run(["powershell", "-NoProfile", "-Command", commande], capture_output=True, text=True)
    return (p.stdout + p.stderr).strip()


class Appli:
    """Une instance de Jumelles, sa page WebView2 et les erreurs de sa console"""

    port = 9222

    def __init__(self, pw, exe=EXE, francais=True):
        # une instance precedente encore en vie garderait le navigateur WebView2, et son port
        for p in psutil.process_iter(["name"]):
            if (p.info["name"] or "").lower() in ("jumelles.exe", "msedgewebview2.exe"):
                try:
                    p.kill()
                except psutil.Error:
                    pass
        time.sleep(1)
        Appli.port += 1
        ps(
            "Set-ItemProperty 'HKLM:\\SOFTWARE\\Policies\\Microsoft\\Edge\\WebView2\\AdditionalBrowserArguments' "
            f"-Name '{exe.name}' -Value '--remote-debugging-port={Appli.port}'"
        )
        self.proc = subprocess.Popen([str(exe)])
        self.pid = self.proc.pid
        try:
            self.ouvrir(pw, francais)
        except Exception:
            ImageGrab.grab().save(SORTIE / f"lancement-{Appli.port}.png")
            try:
                cibles = urllib.request.urlopen(f"http://127.0.0.1:{Appli.port}/json/list", timeout=5).read().decode()
            except Exception as e:
                cibles = str(e)
            R.note(f"lancement en échec, cibles DevTools : {cibles[:600]}")
            self.proc.kill()
            raise

    def ouvrir(self, pw, francais):
        # Playwright rate parfois la page deja ouverte quand il se branche pendant son
        # chargement : il se debranche alors et se rebranche
        for essai in range(4):
            for _ in range(120):
                try:
                    self.navigateur = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{Appli.port}")
                    break
                except Exception:
                    time.sleep(0.5)
            else:
                raise RuntimeError("WebView2 injoignable")
            for _ in range(30):
                pages = [p for c in self.navigateur.contexts for p in c.pages]
                if pages:
                    break
                time.sleep(0.5)
            if pages:
                break
            self.navigateur.close()
        else:
            raise RuntimeError("aucune page dans WebView2")
        if essai:
            R.note(f"page trouvée au branchement {essai + 1} de Playwright")
        self.page = next((p for p in pages if "tauri.localhost" in p.url), pages[0])
        self.erreurs = []
        self.page.on("console", lambda m: m.type == "error" and self.erreurs.append(m.text))
        self.page.on("pageerror", lambda e: self.erreurs.append(str(e)))
        self.page.wait_for_selector(".accueil")
        self.langue = self.page.evaluate("navigator.language")
        if francais:
            # langue de Windows simulee avant le chargement de main.js
            self.page.add_init_script("Object.defineProperty(Navigator.prototype, 'language', {get: () => 'fr-FR'})")
        # rechargement apres la pose des ecouteurs : les erreurs du demarrage sont comptees
        self.page.reload()
        self.page.wait_for_selector(".accueil")

    def ev(self, expr, arg=None):
        return self.page.evaluate(expr, arg)

    def attendre(self, expr, arg=None, timeout=30):
        """Attend qu'une expression soit vraie. Page.wait_for_function l'evaluerait par eval
        dans la page, que la CSP de Jumelles refuse."""
        fin = time.time() + timeout
        while not self.ev(expr, arg):
            if time.time() > fin:
                raise TimeoutError(f"attente trop longue : {expr}")
            time.sleep(0.05)

    def texte(self, cle, *args):
        """Texte de l'interface, calcule par ses propres fonctions"""
        return self.ev("([c, a]) => typeof T[c] === 'function' ? T[c](f, ...a) : T[c]", [cle, list(args)])

    def avis(self):
        return self.ev("[...document.querySelectorAll('.avis li')].map(l => l.textContent)")

    def analyser(self, timeout=900):
        statuts = set()
        debut = time.time()
        self.page.click("[data-action=analyser]")
        while self.ev("etat.enCours"):
            statuts.add(self.ev("etat.statut"))
            time.sleep(0.1)
            if time.time() - debut > timeout:
                raise RuntimeError("analyse trop longue")
        mur = time.time() - debut
        if self.ev("etat.ecran") != "groupes":
            raise RuntimeError(f"analyse en echec : {self.ev('etat.statut')}")
        return self.ev("etat.resultat"), mur, statuts

    def deposer(self, *chemins):
        """Depot de dossiers sur la fenetre, par l'evenement que Tauri emet"""
        self.ev(
            "p => window.__TAURI__.event.emit('tauri://drag-drop', {paths: p, position: {x: 20, y: 20}})",
            [str(c) for c in chemins],
        )
        time.sleep(1)

    def vignettes(self, selecteur, timeout=60):
        """Dimensions des vignettes une fois chargees, (0, 0) pour une vignette en echec"""
        self.attendre("s => [...document.querySelectorAll(s)].every(i => i.complete)", selecteur, timeout)
        return self.ev("s => [...document.querySelectorAll(s)].map(i => [i.naturalWidth, i.naturalHeight])", selecteur)

    def memoire(self):
        """Memoire privee de Jumelles, puis de chaque sorte de processus WebView2, en Mo"""
        try:
            p = psutil.Process(self.pid)
            sortes = Counter()
            for x in p.children(recursive=True):
                sorte = next((a.split("=", 1)[1] for a in x.cmdline() if a.startswith("--type=")), "navigateur")
                sortes[sorte] += x.memory_info().private
            detail = ", ".join(f"{s} {v // 2**20}" for s, v in sortes.most_common())
            return f"{p.memory_info().private // 2**20} Mo pour Jumelles, {sum(sortes.values()) // 2**20} Mo pour WebView2 ({detail})"
        except psutil.Error:
            return "?"

    def fermer(self):
        R.ok(not self.erreurs, "aucune erreur dans la console de l'interface", "; ".join(self.erreurs[:5]))
        try:
            self.navigateur.close()
        except Exception:
            pass
        try:
            enfants = psutil.Process(self.pid).children(recursive=True)
        except psutil.Error:
            enfants = []
        self.proc.kill()
        self.proc.wait()
        for e in psutil.wait_procs(enfants, timeout=15)[1]:
            e.kill()
        time.sleep(1)


def dialogue(pid, timeout=30):
    """Fenetre de dialogue de Windows ouverte par Jumelles"""
    fin = time.time() + timeout
    while time.time() < fin:
        for h in findwindows.find_windows(process=pid, class_name="#32770", visible_only=True):
            return Desktop(backend="uia").window(handle=h)
        time.sleep(0.2)
    raise RuntimeError("aucun dialogue de Windows")


def choisir_dossier(appli, chemin):
    """Choix d'un dossier dans le dialogue de Windows, comme le ferait un utilisateur"""
    d = dialogue(appli.pid)
    d.wait("ready", timeout=15)
    champs = d.descendants(control_type="Edit")
    # champ « Dossier » du dialogue de Windows, 1148 dans sa version Vista, 1152 avant
    champ = next((c for c in champs if c.element_info.automation_id in ("1148", "1152")), None)
    if champ is None:
        raise RuntimeError("champ du dossier introuvable : " + ", ".join(f"{c.element_info.name}/{c.element_info.automation_id}" for c in champs))
    champ.set_edit_text(str(chemin))
    bouton = d.child_window(auto_id="1", control_type="Button")
    bouton.invoke()
    # un premier clic peut ouvrir le dossier tape au lieu de le choisir
    for _ in range(25):
        if not findwindows.find_windows(process=appli.pid, class_name="#32770", visible_only=True):
            return
        time.sleep(0.2)
    bouton.invoke()


def attendre_fin(appli, timeout=300):
    """Attend la fin d'une suppression. Un fichier ouvert ailleurs fait afficher par Windows
    un dialogue : la recette l'ignore (Skip), ou l'annule faute de ce bouton."""
    vus = []
    fin = time.time() + timeout
    while appli.ev("etat.occupe"):
        if time.time() > fin:
            ImageGrab.grab().save(SORTIE / "suppression-bloquee.png")
            fenetres = []
            for h in findwindows.find_windows(visible_only=True):
                e = findwindows.find_element(handle=h)
                if e.process_id == appli.pid or e.name.lower().startswith(("file", "fichier", "delet", "suppr")):
                    fenetres.append(f"{e.class_name} « {e.name} » (processus {e.process_id})")
            raise RuntimeError(f"suppression trop longue ; dialogues vus : {vus} ; fenêtres : {fenetres}")
        for h in findwindows.find_windows(process=appli.pid, visible_only=True):
            try:
                w = Desktop(backend="uia").window(handle=h)
                titre = w.window_text()
                if titre == "Jumelles":
                    continue
                boutons = {b.window_text(): b for b in w.descendants(control_type="Button") if b.window_text()}
            except Exception:
                continue
            vu = f"{titre} : {', '.join(boutons)}"
            if vu not in vus:
                vus.append(vu)
                ImageGrab.grab().save(SORTIE / f"dialogue-{len(vus)}.png")
            for nom in ("Skip", "Ignorer"):
                if nom in boutons:
                    boutons[nom].invoke()
                    break
            else:
                if "In Use" in titre or "en cours d'utilisation" in titre:
                    for nom in ("Cancel", "Annuler"):
                        if nom in boutons:
                            boutons[nom].invoke()
        time.sleep(0.3)
    return vus


def corbeille():
    """Chemins d'origine des fichiers dans la corbeille de l'utilisateur, d'apres les $I,
    en minuscules"""
    sid = subprocess.run(["whoami", "/user", "/fo", "csv", "/nh"], capture_output=True, text=True).stdout
    sid = sid.strip().split(",")[-1].strip('"')
    chemins = set()
    for lecteur in ("C:", "D:"):
        for f in Path(f"{lecteur}\\$Recycle.Bin\\{sid}").glob("$I*"):
            b = f.read_bytes()
            if int.from_bytes(b[:8], "little") == 2:
                n = int.from_bytes(b[24:28], "little")
                chemins.add(b[28 : 28 + 2 * n].decode("utf-16-le").rstrip("\0").lower())
            else:
                chemins.add(b[24 : 24 + 520].decode("utf-16-le").split("\0")[0].lower())
    return chemins


def sans_prefixe(chemin):
    return chemin[4:] if chemin.startswith("\\\\?\\") else chemin


def retrouver(chemin, enregistres):
    """Chemin sous lequel la corbeille garde ce fichier : le meme, ou une autre ecriture du
    meme dossier (nom court 8.3, prefixe \\?\\)"""
    if chemin.lower() in enregistres:
        return chemin.lower()
    nom = os.path.basename(chemin).lower()
    dossier = os.path.normcase(sans_prefixe(os.path.realpath(win(os.path.dirname(chemin)))))
    for e in enregistres:
        if os.path.basename(e) == nom:
            try:
                if os.path.normcase(sans_prefixe(os.path.realpath(sans_prefixe(os.path.dirname(e))))) == dossier:
                    return e
            except OSError:
                pass
    return None


def win(chemin):
    """Chemin lisible au-dela de 260 caracteres"""
    return "\\\\?\\" + chemin if os.name == "nt" and len(chemin) > 240 and not chemin.startswith("\\\\") else chemin


def empreinte(chemin):
    return hashlib.sha1(Path(win(chemin)).read_bytes()).hexdigest()


# Formats qui dependent des extensions de Windows : Windows dit lui-meme, par WIC, s'il les
# decode, et en quelle taille. Jumelles doit decoder exactement ceux-la.
SPECIAUX = RAW | {".heic", ".heif", ".webp"}
WINDOWS = {}

LIRE = r"""
param($liste)
Add-Type -AssemblyName PresentationCore
foreach ($p in Get-Content -LiteralPath $liste -Encoding UTF8) {
  try {
    $s = [IO.File]::OpenRead($p)
    try {
      $f = [Windows.Media.Imaging.BitmapFrame]::Create($s, [Windows.Media.Imaging.BitmapCreateOptions]::None, [Windows.Media.Imaging.BitmapCacheOption]::OnLoad)
      "$($f.PixelWidth) $($f.PixelHeight)"
    } finally { $s.Dispose() }
  } catch { "0 0" }
}
"""


def lire_par_windows():
    """Decode par WPF, donc par WIC, les fichiers du jeu dont le format depend d'une extension"""
    rels = [r for r in ATTENDU if Path(r).suffix.lower() in SPECIAUX]
    liste, script = SORTIE / "speciaux.txt", SORTIE / "lire.ps1"
    liste.write_text("\n".join(str(JEU / r) for r in rels), encoding="utf-8")
    script.write_text(LIRE, encoding="utf-8")
    sortie = subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(script), str(liste)],
        capture_output=True, text=True,
    ).stdout.split("\n")
    WINDOWS.clear()
    for rel, ligne in zip(rels, sortie):
        w, h = (int(x) for x in ligne.split())
        WINDOWS[rel] = (w, h) if w else None
    lus = Counter(Path(r).suffix.lower() for r, d in WINDOWS.items() if d)
    tous = Counter(Path(r).suffix.lower() for r in WINDOWS)
    R.note("Windows décode : " + ", ".join(f"{e} {lus[e]} sur {n}" for e, n in sorted(tous.items())))
    R.bloc("\n".join(f"{r} : {'%d x %d' % d if d else 'non décodé'}" for r, d in sorted(WINDOWS.items()) if r.startswith("formats/")))


def decodable(rel):
    if ATTENDU[rel]["variante"] in ("vide", "texte", "ignore"):
        return False
    if Path(rel).suffix.lower() in SPECIAUX:
        return WINDOWS.get(rel) is not None
    return True


def verifier_groupes(resultat, prefixe, cle, titre):
    """Compare les groupes trouves aux scenes du jeu ; cle(chemin de Jumelles) -> fichier du jeu"""
    fichiers = {rel: v for rel, v in ATTENDU.items() if rel.startswith(prefixe) and v["variante"] != "ignore"}
    scenes = defaultdict(set)
    for rel, v in fichiers.items():
        if v["scene"] and decodable(rel) and v["variante"] not in INDICATIVES | {"lien-dur"}:
            scenes[v["scene"]].add(rel)
    attendus = {s: m for s, m in scenes.items() if len(m) > 1}

    groupes = [[(cle(m["chemin"]), m) for m in g] for g in resultat["groupes"]]
    inconnus = [k for g in groupes for k, _ in g if k not in fichiers]
    R.ok(not inconnus, f"{titre} : tout fichier des groupes vient du jeu", ", ".join(inconnus[:5]))
    groupe_de = {k: i for i, g in enumerate(groupes) for k, _ in g}

    # scenes retrouvees entieres, et taux par variante
    par_variante = defaultdict(lambda: [0, 0])
    manques = []
    for s, membres in attendus.items():
        ref = next((m for m in membres if fichiers[m]["variante"] in ("original", "raw")), sorted(membres)[0])
        for m in membres - {ref}:
            v = fichiers[m]["variante"]
            par_variante[v][1] += 1
            if m in groupe_de and groupe_de.get(m) == groupe_de.get(ref):
                par_variante[v][0] += 1
            else:
                manques.append(f"{m} ({v})")
    lignes = [f"{v} : {t} sur {n}" for v, (t, n) in sorted(par_variante.items())]
    R.bloc(f"{titre}, retrouvées avec leur original :\n" + "\n".join(lignes))
    R.ok(not manques, f"{titre} : chaque variante est dans le groupe de son original", "; ".join(manques[:8]))

    # aucun groupe ne mele deux scenes
    meles = []
    for g in groupes:
        sc = {fichiers[k]["scene"] for k, _ in g if k in fichiers}
        if len(sc) > 1:
            meles.append(" + ".join(k for k, _ in g))
    R.ok(not meles, f"{titre} : aucun groupe ne mêle deux photos différentes", " | ".join(meles[:5]))

    # variantes sans verdict
    for rel, v in fichiers.items():
        if v["variante"] in INDICATIVES and v["scene"]:
            ref = next((m for m in scenes.get(v["scene"], ()) if fichiers[m]["variante"] in ("original", "raw")), None)
            avec = ref is not None and rel in groupe_de and groupe_de[rel] == groupe_de.get(ref)
            R.note(f"{titre} : {rel} ({v['variante']}) {'retrouvée' if avec else 'non retrouvée'}")

    # drapeau identique, reference en tete, dimensions
    erreurs_ident, erreurs_ref, erreurs_dim, sans = [], [], [], []
    for g in groupes:
        ref_k, ref_m = g[0]
        ref_h = empreinte(ref_m["chemin"])
        for k, m in g[1:]:
            if (empreinte(m["chemin"]) == ref_h) != m["identique"]:
                erreurs_ident.append(k)
        attendue = max(g, key=lambda x: (x[1]["largeur"] * x[1]["hauteur"], -x[1]["modifie"]))
        if attendue[1]["largeur"] * attendue[1]["hauteur"] > ref_m["largeur"] * ref_m["hauteur"] or (
            attendue[1]["largeur"] * attendue[1]["hauteur"] == ref_m["largeur"] * ref_m["hauteur"]
            and attendue[1]["modifie"] < ref_m["modifie"]
        ):
            erreurs_ref.append(f"{ref_k} avant {attendue[0]}")
        for k, m in g:
            if not m["largeur"]:
                sans.append(k)
                continue
            if Path(k).suffix.lower() in SPECIAUX:
                taille = WINDOWS.get(k)
            else:
                with Image.open(win(m["chemin"])) as im:
                    taille = im.size
            if taille != (m["largeur"], m["hauteur"]):
                erreurs_dim.append(f"{k} : {m['largeur']} x {m['hauteur']} au lieu de {taille}")
    R.ok(not erreurs_ident, f"{titre} : « identique » vaut pour les seules copies octet pour octet", ", ".join(erreurs_ident[:5]))
    R.ok(not erreurs_ref, f"{titre} : la photo gardée est la plus grande, puis la plus ancienne", ", ".join(erreurs_ref[:5]))
    R.ok(not erreurs_dim, f"{titre} : dimensions affichées", ", ".join(erreurs_dim[:5]))
    if sans:
        R.note(f"{titre} : groupées sans dimensions (copies exactes d'une photo non décodée) : {', '.join(sans)}")
    return groupes, fichiers


def cle_jeu(chemin):
    return os.path.relpath(chemin, JEU).replace("\\", "/")


def avis_attendus(appli, resultat):
    """Messages de fin d'analyse que l'interface doit afficher"""
    a = []
    if resultat["illisibles"]:
        a.append(appli.texte("illisibles", resultat["illisibles"]))
    n = resultat["nonDecodees"]
    for cle in ("heif", "raw", "autres"):
        if n[cle]:
            a.append(appli.texte(cle, n[cle]))
    return a


def etape(titre, fonction, *args):
    """Partie d'un scenario : son echec est rapporte, la suite continue depuis la vue
    d'ensemble"""
    try:
        return fonction(*args)
    except Exception:
        R.ok(False, f"{titre} va jusqu'au bout", traceback.format_exc().strip().splitlines()[-1])
        R.bloc(traceback.format_exc())
        a = args[0] if args and isinstance(args[0], Appli) else None
        if a:
            try:
                a.ev("""() => {
                    document.getElementById('zoom').hidden = true;
                    if (document.getElementById('confirmer').open) document.getElementById('confirmer').close();
                    if (etat.resultat && !etat.occupe) { etat.ecran = 'groupes'; rendre(); }
                }""")
            except Exception:
                pass


def non_lus(prefixe, extensions):
    """Fichiers du jeu de ces formats que Windows ne decode pas"""
    return sum(1 for r in ATTENDU if r.startswith(prefixe) and Path(r).suffix.lower() in extensions and not decodable(r))


def scenario_principal(pw):
    R.section("Disque fixe : ajout, analyse, tri, corbeille")
    a = Appli(pw)
    try:
        R.ok(a.ev("T.code") == "fr", "interface en français", f"navigator.language d'origine : {a.langue}")
        R.capture(a.page, "01-accueil")

        a.page.click("[data-action=ajouter]")
        choisir_dossier(a, JEU / "photos")
        a.attendre("etat.dossiers.length === 1", timeout=10)
        R.ok(a.ev("etat.dossiers") == [str(JEU / "photos")], "dossier choisi dans le dialogue de Windows", a.ev("etat.dossiers"))

        a.deposer(JEU / "photos" / "Sauvegarde", JEU / "anglais")
        R.ok(len(a.ev("etat.dossiers")) == 3, "deux dossiers déposés sur la fenêtre", a.ev("etat.dossiers"))
        a.page.click("[data-retirer='2']")
        R.ok(len(a.ev("etat.dossiers")) == 2, "un dossier retiré de la liste")
        R.capture(a.page, "02-dossiers")

        resultat, mur, statuts = a.analyser()
        R.note(f"première analyse : {resultat['photos']} photos en {resultat['dureeMs'] / 1000:.1f} s (fenêtre : {mur:.1f} s)")
        R.note("étapes affichées : " + " | ".join(sorted(s for s in statuts if s)))
        images = [rel for rel, v in ATTENDU.items() if rel.startswith("photos/") and v["variante"] != "ignore"]
        R.ok(resultat["photos"] == len(images), "toutes les photos du dossier sont trouvées, sous-dossier imbriqué compris une seule fois",
             f"{resultat['photos']} pour {len(images)}")
        groupes, fichiers = verifier_groupes(resultat, "photos/", cle_jeu, "Disque fixe")
        lien = "photos/Divers/lien-dur.jpg"
        R.ok(not any(k == lien or k == "photos/Originaux/IMG_1103.JPG" for g in groupes for k, _ in g),
             "un lien dur vers une photo n'en fait pas une copie")
        nd = resultat["nonDecodees"]
        R.note(f"non décodées : HEIC {nd['heif']}, RAW {nd['raw']}, autres {nd['autres']} ; illisibles : {resultat['illisibles']}")
        heic, raws = non_lus("photos/", {".heic", ".heif"}), non_lus("photos/", RAW)
        R.ok(nd["heif"] == heic, "HEIC non lus comptés à part, comme Windows", f"{nd['heif']} pour {heic}")
        R.ok(nd["raw"] == raws, "RAW non lus comptés à part, comme Windows", f"{nd['raw']} pour {raws}")
        R.ok(resultat["illisibles"] == (1 if os.name == "nt" else 0), "un dossier que Windows refuse de lister compte comme illisible", str(resultat["illisibles"]))
        R.ok(a.avis() == avis_attendus(a, resultat), "messages de fin d'analyse", " | ".join(a.avis()))
        R.capture(a.page, "03-ensemble")

        etape("Fenêtre et clavier", fenetre_et_clavier, a)

        etape("Onglets et cache", onglets_et_cache, a, resultat)
        etape("Page de groupe", page_de_groupe, a)
        etape("Corbeille", suppression_corbeille, a)
    finally:
        a.fermer()


def onglets_et_cache(a, resultat):
    # onglets
    for onglet in ("identiques", "proches", "aVerifier", "tous"):
        a.page.click(f"[data-onglet={onglet}]")
        compte = a.ev("o => etat.groupes.filter(filtres[o]).length", onglet)
        lignes = a.ev("document.querySelectorAll('.ligne').length")
        affiche = a.ev("document.querySelector('.onglet.actif .num').textContent")
        R.ok(lignes == compte and re.sub(r"\D", "", affiche) == str(compte), f"onglet {onglet} : {compte} groupes")
    R.ok(a.ev("etat.groupes.filter(filtres.identiques).length + etat.groupes.filter(filtres.proches).length === etat.groupes.length"),
         "Identiques et Proches couvrent tous les groupes")
    premieres = a.vignettes(".ligne:nth-child(-n+4) img")
    R.ok(all(w > 0 for w, _ in premieres), "vignettes de la vue d'ensemble", str(premieres[:4]))

    # nouvelle analyse, empreintes reprises du cache
    a.page.click("[data-action=accueil]")
    resultat2, mur2, _ = a.analyser()
    R.note(f"seconde analyse : {resultat2['dureeMs'] / 1000:.1f} s (fenêtre : {mur2:.1f} s)")
    R.ok(resultat2["dureeMs"] < resultat["dureeMs"] / 2, "le cache accélère la seconde analyse",
         f"{resultat['dureeMs']} ms puis {resultat2['dureeMs']} ms")
    R.ok(len(resultat2["groupes"]) == len(resultat["groupes"]), "mêmes groupes à la seconde analyse")
    cache = EXE.with_name("jumelles.cache")
    R.ok(cache.exists() and cache.stat().st_size > 0, "cache écrit à côté de l'exe", f"{cache.stat().st_size if cache.exists() else 0} octets")


def fenetre_et_clavier(a):
    # fenetre a l'ouverture, sur l'ecran du runner
    h = findwindows.find_windows(process=a.pid, title="Jumelles")[0]
    rect = win32gui.GetWindowRect(h)
    travail = win32api.GetMonitorInfo(win32api.MonitorFromWindow(h))["Work"]
    placement = win32gui.GetWindowPlacement(h)
    R.note(f"fenêtre {rect[2] - rect[0]} x {rect[3] - rect[1]} en ({rect[0]}, {rect[1]}), "
           f"{'agrandie' if placement[1] == 3 else 'non agrandie'}, zone de travail de l'écran {travail[2]} x {travail[3]}")
    gauche, haut = win32gui.ClientToScreen(h, (0, 0))
    largeur, hauteur = win32gui.GetClientRect(h)[2:]
    R.ok(gauche >= travail[0] and haut >= travail[1] and gauche + largeur <= travail[2] and haut + hauteur <= travail[3],
         "la fenêtre tient dans la zone de travail de l'écran", f"intérieur {largeur} x {hauteur} en ({gauche}, {haut})")
    ImageGrab.grab().save(SORTIE / "bureau.png")
    R.lignes.append("- capture : `bureau.png`")

    # clavier dans la vue d'ensemble : fleches puis Entree
    a.page.focus(".ligne >> nth=0")
    actif = "() => document.activeElement?.dataset?.ouvrir ?? document.activeElement?.tagName"
    suivi = [a.ev(actif)]
    for touche in ("ArrowDown", "ArrowDown"):
        a.page.keyboard.press(touche)
        suivi.append(a.ev(actif))
    a.page.keyboard.press("Enter")
    R.ok(a.ev("etat.ecran") == "groupe" and a.ev("etat.courant") == 2, "flèches et Entrée ouvrent le troisième groupe",
         f"focus {suivi}, écran {a.ev('etat.ecran')}, groupe {a.ev('etat.courant')}")
    if a.ev("etat.ecran") == "groupe":
        a.page.keyboard.press("Escape")

    # fenetre a sa taille minimale (720 x 480 points), comme apres un redimensionnement
    try:
        win32gui.ShowWindow(h, 9)  # SW_RESTORE : une fenetre agrandie ne se redimensionne pas
        time.sleep(1)
        r = win32gui.GetWindowRect(h)
        bordure = (r[2] - r[0] - a.ev("innerWidth"), r[3] - r[1] - a.ev("innerHeight"))
        win32gui.MoveWindow(h, 0, 0, 720 + bordure[0], 480 + bordure[1], True)
        time.sleep(1)
        visible = "() => { const r = document.getElementById('corbeille').getBoundingClientRect(); return r.width > 0 && r.bottom <= innerHeight && r.right <= innerWidth }"
        R.note(f"taille minimale : {a.ev('[innerWidth, innerHeight]')}")
        R.ok(a.ev(visible), "taille minimale : bouton de la corbeille visible dans la vue d'ensemble")
        R.capture(a.page, "03-ensemble-minimale")
        a.page.click(".ligne >> nth=0")
        a.vignettes(".photo img")
        R.ok(a.ev(visible), "taille minimale : bouton de la corbeille visible dans un groupe")
        R.capture(a.page, "04-groupe-minimale")
        a.page.keyboard.press("Escape")
    finally:
        win32gui.SetWindowPlacement(h, placement)
        time.sleep(1)


def page_de_groupe(a):
    R.section("Page de groupe et clavier")
    a.page.click("[data-onglet=proches]")
    a.page.click(".ligne >> nth=0")
    n = a.ev("liste().length")
    titre = a.page.text_content(".page-tete h1")
    R.ok(titre == a.texte("groupeNumero", 1, n), "titre de la page de groupe", titre)
    g = a.ev("liste()[etat.courant].membres")
    marques = a.ev("[...document.querySelectorAll('.photo')].map(p => p.classList.contains('supprimee'))")
    R.ok(marques == [False] + [True] * (len(g) - 1), "la première photo est gardée, les autres marquées", str(marques))
    tailles = a.vignettes(".photo img")
    R.ok(all(w > 0 for w, _ in tailles), "photos du groupe affichées", str(tailles))
    R.capture(a.page, "04-groupe")

    a.page.keyboard.press("ArrowRight")
    R.ok(a.page.text_content(".page-tete h1") == a.texte("groupeNumero", 2, n), "flèche droite : groupe suivant")
    a.page.keyboard.press("ArrowLeft")
    R.ok(a.page.text_content(".page-tete h1") == a.texte("groupeNumero", 1, n), "flèche gauche : groupe précédent")
    a.page.keyboard.press("2")
    ids = [m["id"] for m in g]
    marquees = a.ev("ids => ids.map(i => etat.marques.has(i))", ids)
    R.ok(marquees == [i != 1 for i in range(len(ids))], "touche 2 : seule la deuxième photo est gardée", str(marquees))
    a.page.keyboard.press("1")
    a.page.keyboard.press(" ")
    visible = a.ev("!document.getElementById('zoom').hidden")
    zoom = a.vignettes("#zoom-image")
    R.ok(visible and zoom[0][0] > 0, "Espace : photo agrandie", str(zoom))
    R.capture(a.page, "05-agrandie")
    avant = {p.pid for p in psutil.process_iter()}
    a.page.dblclick("#zoom-image")
    time.sleep(5)
    lances = [p for p in psutil.process_iter(["name"]) if p.pid not in avant]
    R.note("double-clic sur la photo agrandie, programmes lancés : " + (", ".join(sorted({p.info["name"] for p in lances})) or "aucun"))
    for p in lances:
        if p.info["name"] and p.info["name"].lower() not in ("conhost.exe", "msedgewebview2.exe", "powershell.exe"):
            try:
                p.kill()
            except psutil.Error:
                pass
    a.page.keyboard.press("Escape")
    R.ok(a.ev("document.getElementById('zoom').hidden"), "Échap ferme l'agrandissement")
    a.page.click(".menu li >> nth=2")
    R.ok(a.ev("etat.courant") == 2, "menu des groupes : aller au troisième")
    a.page.keyboard.press("Escape")
    R.ok(a.ev("etat.ecran") == "groupes", "Échap revient à la vue d'ensemble")

    # un groupe garde au moins une photo
    i = a.ev("liste().findIndex(g => g.membres.length === 2)")
    if i >= 0:
        a.ev("i => aller(i)", i)
        g = a.ev("liste()[etat.courant].membres")
        a.page.click(f"[data-supprimer='{g[0]['id']}']")
        R.ok(a.ev("id => !etat.marques.has(id)", g[0]["id"]) and a.page.text_content("#message") == a.texte("auMoinsUne"),
             "un groupe garde au moins une photo", a.page.text_content("#message"))
        a.page.click(f"[data-garder='{g[1]['id']}']")
        a.page.click(f"[data-supprimer='{g[0]['id']}']")
        R.ok(a.ev("ids => etat.marques.has(ids[0]) && !etat.marques.has(ids[1])", [g[0]["id"], g[1]["id"]]),
             "garder l'autre photo permet de marquer la première")
        a.page.keyboard.press("1")
        a.page.keyboard.press("Escape")
    vus = a.ev("etat.groupes.filter(g => g.vu && !filtres.identiques(g)).length")
    a.page.click("[data-onglet=aVerifier]")
    reste = a.ev("document.querySelectorAll('.ligne').length")
    proches = a.ev("etat.groupes.filter(filtres.proches).length")
    R.ok(reste == proches - vus, "les groupes ouverts quittent l'onglet À vérifier", f"{vus} vus, {reste} restent")

    # onglet A verifier : la fleche droite parcourt ses groupes dans l'ordre (#25)
    n = a.ev("liste().length")
    a.page.click(".ligne >> nth=0")
    parcours = []
    for _ in range(min(n, 8)):
        parcours.append((a.page.text_content(".page-tete h1"), a.ev("liste()[etat.courant]?.membres[0].id")))
        a.page.keyboard.press("ArrowRight")
    titres = [a.texte("groupeNumero", k + 1, n) for k in range(len(parcours))]
    R.ok([t for t, _ in parcours] == titres and len({i for _, i in parcours}) == len(parcours),
         "onglet À vérifier : la flèche droite parcourt ses groupes dans l'ordre", " | ".join(t for t, _ in parcours))
    a.page.keyboard.press("Escape")
    a.page.click("[data-onglet=tous]")

    # orientation EXIF : la vignette de la variante tournee par etiquette est droite
    rel = next((k for k, v in ATTENDU.items() if k.startswith("photos/") and v["variante"] == "orientation"), None)
    idx = a.ev("c => etat.groupes.findIndex(g => g.membres.some(m => m.chemin.endsWith(c)))", rel.split("/")[-1])
    if idx >= 0:
        a.ev("i => { etat.onglet = 'tous'; aller(i) }", idx)
        dims = a.vignettes(".photo img")
        membres = a.ev("liste()[etat.courant].membres.map(m => m.chemin)")
        k = next(j for j, c in enumerate(membres) if c.endswith(rel.split("/")[-1]))
        sens = lambda d: d[0] >= d[1]
        R.ok(sens(dims[k]) == sens(dims[0]), "vignette d'une photo tournée par l'étiquette EXIF affichée droite", str(dims))
        R.capture(a.page, "06-orientation")
        a.page.keyboard.press("Escape")


def suppression_corbeille(a):
    R.section("Corbeille")
    marques = a.ev("[...etat.marques]")
    chemins = {i: c for i, c in a.ev("etat.groupes.flatMap(g => g.membres).map(m => [m.id, m.chemin])")}
    tous = list(chemins.values())
    a_suppr = [chemins[i] for i in marques]
    modifiee, ouverte = a_suppr[0], a_suppr[1]
    os.utime(modifiee, (time.time(), time.time()))
    verrou = open(ouverte, "rb")  # ouvert sans partage de suppression, comme par un autre programme
    try:
        a.page.click("#corbeille")
        R.ok(a.ev("document.getElementById('confirmer').open"), "confirmation avant la corbeille")
        titre = a.page.text_content("#confirmer-titre")
        R.ok(titre == a.texte("confirmerTitre", len(marques)), "titre de la confirmation", titre)
        R.capture(a.page, "07-confirmation")
        a.page.click("#confirmer-non")
        R.ok(all(Path(c).exists() for c in a_suppr), "Annuler ne touche à rien")
        a.page.click("#corbeille")
        debut = time.time()
        a.page.click("#confirmer-oui")
        vus = attendre_fin(a)
        R.note(f"suppression de {len(marques)} photos en {time.time() - debut:.1f} s")
        R.note("dialogues de Windows : " + (" ; ".join(vus) or "aucun"))
    finally:
        verrou.close()
    partis = [c for c in a_suppr if c not in (modifiee, ouverte)]
    dans = corbeille()
    absents = [c for c in partis if Path(win(c)).exists()]
    trouves = {c: retrouver(c, dans) for c in partis}
    pas_dans = [c for c, e in trouves.items() if e is None]
    for c, e in trouves.items():
        if e and e != c.lower():
            R.note(f"{os.path.basename(c)} est dans la corbeille sous le chemin {e}")
    for c in pas_dans:
        nom = os.path.basename(c).lower()
        R.note(f"{nom} : entrées de la corbeille au même nom : {[e for e in dans if os.path.basename(e) == nom]}")
    R.ok(not absents, "les photos marquées ont quitté leur dossier", ", ".join(absents[:5]))
    R.ok(not pas_dans, "elles sont dans la corbeille de Windows", ", ".join(pas_dans[:5]))
    R.ok(Path(modifiee).exists(), "la photo modifiée depuis l'analyse est restée")
    R.ok(Path(ouverte).exists(), "la photo ouverte dans un autre programme est restée")
    gardees = [c for c in tous if c not in a_suppr]
    R.ok(all(Path(c).exists() for c in gardees), "les photos gardées sont toutes là")
    attendus = [a.texte("envoyees", len(partis)), a.texte("modifiees", 1), a.texte("restees", 1)]
    R.ok(a.avis()[:3] == attendus, "bilan affiché", " | ".join(a.avis()))
    R.capture(a.page, "08-bilan")


def lecteur_sans_corbeille(pw, titre, racine, dossiers, destination, ajout_par_dialogue=True):
    """Analyse d'un lecteur reseau ou amovible, corbeille refusee, deplacement"""
    a = Appli(pw)
    try:
        if ajout_par_dialogue:
            a.page.click("[data-action=ajouter]")
            choisir_dossier(a, dossiers)
        else:
            a.deposer(dossiers)
        resultat, mur, _ = a.analyser()
        R.note(f"{titre} : {resultat['photos']} photos en {resultat['dureeMs'] / 1000:.1f} s")
        cle = lambda c: racine + "/" + c[len(dossiers) :].lstrip("\\").replace("\\", "/")
        verifier_groupes(resultat, racine, cle, titre)
        marques = a.ev("[...etat.marques]")
        chemins = dict(a.ev("etat.groupes.flatMap(g => g.membres).map(m => [m.id, m.chemin])"))
        a_suppr = [chemins[i] for i in marques]
        a.page.click("#corbeille")
        a.page.click("#confirmer-oui")
        attendre_fin(a)
        R.ok(all(Path(c).exists() for c in a_suppr), f"{titre} : la corbeille ne supprime rien sur ce lecteur")
        R.ok(a.texte("sansCorbeille", len(a_suppr)) in a.avis(), f"{titre} : message du lecteur sans corbeille", " | ".join(a.avis()))
        R.ok(a.ev("etat.marques.size") == len(a_suppr), f"{titre} : les photos restent marquées")
        R.capture(a.page, f"09-{titre.lower().replace(' ', '-')}-sans-corbeille")

        destination.mkdir(parents=True, exist_ok=True)
        avant = {p.name for p in destination.iterdir()}
        a.page.click("#deplacer")
        choisir_dossier(a, destination)
        attendre_fin(a)
        a.attendre("etat.marques.size === 0", timeout=60)
        R.ok(not any(Path(c).exists() for c in a_suppr), f"{titre} : les photos ont quitté le lecteur")
        arrivees = sorted({p.name for p in destination.iterdir()} - avant)
        R.note(f"{titre} : arrivées dans {destination} : {', '.join(arrivees)}")
        R.ok(len(arrivees) == len(a_suppr), f"{titre} : une arrivée par photo, sans écraser", f"{len(arrivees)} pour {len(a_suppr)}")
        R.ok(a.texte("deplacees", len(a_suppr), str(destination)) in a.avis(), f"{titre} : bilan du déplacement", " | ".join(a.avis()))
        R.capture(a.page, f"10-{titre.lower().replace(' ', '-')}-deplacees")
        return arrivees
    finally:
        a.fermer()


def scenario_reseau(pw):
    R.section("Partage réseau")
    partage = JEU / "reseau"
    R.note(ps(f"New-SmbShare -Name reseau -Path '{partage}' -FullAccess $env:USERNAME | Out-Null; "
              "net use R: \\\\localhost\\reseau /persistent:no; [IO.DriveInfo]::new('R').DriveType"))
    # un fichier deja la porte le nom d'une photo deplacee
    dest = Path("C:/recette/deplacees-reseau")
    dest.mkdir(parents=True, exist_ok=True)
    (dest / "IMG_2001.JPG").write_bytes(b"deja la")
    arrivees = lecteur_sans_corbeille(pw, "Lecteur réseau", "reseau/lettre", "R:\\lettre", dest)
    R.ok({"IMG_2000.JPG", "IMG_2000 (2).JPG", "IMG_2001 (2).JPG"} <= set(arrivees), "noms déjà pris suffixés (2)", ", ".join(arrivees))
    R.ok((dest / "IMG_2001.JPG").read_bytes() == b"deja la", "le fichier déjà présent n'est pas écrasé")
    lecteur_sans_corbeille(pw, "Chemin UNC", "reseau/unc", "\\\\localhost\\reseau\\unc", Path("C:/recette/deplacees-unc"), False)


def scenario_amovible(pw):
    R.section("Support amovible")
    if not shutil.which("imdisk"):
        R.ok(False, "ImDisk installé pour simuler une clé USB")
        return
    R.note(subprocess.run('imdisk -a -s 512M -m U: -o rem -p "/fs:fat32 /q /y"', shell=True, capture_output=True, text=True).stdout.strip()[-200:])
    R.note("type du lecteur U: : " + ps("[IO.DriveInfo]::new('U').DriveType; [IO.DriveInfo]::new('U').DriveFormat").replace("\n", ", "))
    subprocess.run(["robocopy", str(JEU / "amovible"), "U:\\", "/E", "/COPY:DAT", "/DCOPY:T", "/NFL", "/NDL", "/NJH", "/NJS"])
    lecteur_sans_corbeille(pw, "Clé USB", "amovible", "U:\\", Path("C:/recette/deplacees-usb"))


def scenario_cache(pw):
    R.section("Cache dans un dossier en lecture seule")
    dossier = Path("C:/recette/lecture-seule")
    dossier.mkdir(parents=True, exist_ok=True)
    exe = dossier / EXE.name
    shutil.copy(EXE, exe)
    subprocess.run(["icacls", str(dossier), "/deny", "*S-1-1-0:(WD,AD)"], capture_output=True)
    local = Path(os.environ["LOCALAPPDATA"]) / "Jumelles" / "jumelles.cache"
    local.unlink(missing_ok=True)
    a = Appli(pw, exe)
    try:
        a.deposer(JEU / "anglais")
        a.analyser()
    finally:
        a.fermer()
    R.ok(not (dossier / "jumelles.cache").exists(), "rien n'est écrit à côté de l'exe")
    R.ok(local.exists() and local.stat().st_size > 0, "cache écrit dans %LOCALAPPDATA%\\Jumelles")


def scenario_anglais(pw):
    R.section("Interface en anglais")
    a = Appli(pw, francais=False)
    try:
        R.ok(a.ev("T.code") == "en", "anglais quand Windows n'est pas en français", a.langue)
        a.deposer(JEU / "anglais")
        resultat, _, _ = a.analyser()
        verifier_groupes(resultat, "anglais/", cle_jeu, "Anglais")
        R.ok(a.avis() == avis_attendus(a, resultat), "messages en anglais", " | ".join(a.avis()))
        ecrans = {}
        ecrans["ensemble"] = a.ev("document.body.innerText")
        R.capture(a.page, "11-anglais-ensemble")
        a.page.click(".ligne >> nth=0")
        a.vignettes(".photo img")
        ecrans["groupe"] = a.ev("document.body.innerText")
        R.capture(a.page, "12-anglais-groupe")
        a.page.click("#corbeille")
        ecrans["confirmation"] = a.ev("document.getElementById('confirmer').innerText")
        R.capture(a.page, "13-anglais-confirmation")
        a.page.click("#confirmer-non")
        for nom, texte in ecrans.items():
            R.ok(not re.search(r"undefined|NaN|\[object|null", texte), f"textes complets : {nom}")
            R.ok(not re.search(r"[àâçéèêëîïôûùœ]|Groupe|Gardée|corbeille", texte), f"aucun texte français : {nom}")
    finally:
        a.fermer()


def scenario_volume(pw):
    R.section("Volume : 10 000 fichiers")
    a = Appli(pw)
    try:
        a.deposer(JEU / "volume")
        resultat, mur, _ = a.analyser(timeout=1800)
        R.note(f"analyse : {resultat['photos']} fichiers en {resultat['dureeMs'] / 1000:.1f} s, mémoire privée : {a.memoire()}")
        groupes, _ = verifier_groupes(resultat, "volume/", cle_jeu, "Volume")
        tailles = Counter(len(g) for g in groupes)
        R.ok(len(groupes) == 3000 and tailles == Counter({3: 3000}), "3 000 groupes de trois", str(dict(tailles)))
        rendu = a.ev("() => { const t = performance.now(); rendre(); return performance.now() - t }")
        R.note(f"vue d'ensemble : {rendu:.0f} ms pour construire 3 000 lignes")
        onglet = a.ev("() => { const t = performance.now(); etat.onglet = 'proches'; rendre(); etat.onglet = 'tous'; rendre(); return performance.now() - t }")
        R.note(f"deux changements d'onglet : {onglet:.0f} ms")
        a.page.click(".ligne >> nth=0")
        durees = []
        for _ in range(20):
            t = time.time()
            a.page.keyboard.press("ArrowRight")
            a.attendre("n => etat.courant === n", len(durees) + 1)
            durees.append((time.time() - t) * 1000)
        moyenne = sum(durees) / len(durees)
        R.note(f"groupe suivant au clavier, aller-retour du protocole DevTools compris : {moyenne:.0f} ms en moyenne, {max(durees):.0f} ms au plus")
        # dans la page : changement de groupe et mise en page, sans l'aller-retour
        page_seule = a.ev("""() => {
            const d = [];
            for (let k = 0; k < 20; k++) {
                const t = performance.now();
                aller(etat.courant + 1);
                document.body.getBoundingClientRect();
                d.push(performance.now() - t);
            }
            return d;
        }""")
        moyenne = sum(page_seule) / len(page_seule)
        R.note(f"groupe suivant dans la page : {moyenne:.0f} ms en moyenne, {max(page_seule):.0f} ms au plus")
        R.ok(moyenne < 100, "passage d'un groupe à l'autre sous 100 ms avec 3 000 groupes", f"{moyenne:.0f} ms")
        t = time.time()
        a.page.keyboard.press("2")
        R.note(f"marque au clavier : {(time.time() - t) * 1000:.0f} ms")
        R.note(f"mémoire privée après navigation : {a.memoire()}")
        # images decodees gardees en cache par Chromium : rendues quand la memoire manque
        cdp = a.page.context.new_cdp_session(a.page)
        cdp.send("Memory.simulatePressureNotification", {"level": "critical"})
        cdp.send("HeapProfiler.collectGarbage")
        time.sleep(3)
        R.note(f"mémoire privée après une alerte de mémoire simulée : {a.memoire()}")
        R.capture(a.page, "14-volume-groupe")
    finally:
        a.fermer()


def scenario_extensions(pw):
    R.section("Extensions HEIF, HEVC, RAW et WebP du Microsoft Store")
    for f in sorted(EXTENSIONS.glob("*.appx")):
        R.note(f"{f.name.split('_')[0]} : " + (ps(f"Add-AppxPackage -Path '{f}'") or "installée"))
    paquets = defaultdict(list)
    for f in EXTENSIONS.glob("*bundle"):
        paquets[f.name.split("_")[0]].append(f)
    # la version la plus recente que ce Windows accepte
    for nom, versions in sorted(paquets.items()):
        refus = []
        for f in sorted(versions, key=lambda f: [int(x) for x in f.name.split("_")[1].split(".")], reverse=True):
            sortie = ps(f"Add-AppxPackage -Path '{f}'")
            if not sortie:
                break
            refus.append(f"{f.name.split('_')[1]} : {sortie.splitlines()[0][:160]}")
        else:
            f = None
        R.ok(f is not None, f"installation de {nom}" + (f" {f.name.split('_')[1]}" if f else ""), " ; ".join(refus))
    R.bloc(ps("Get-AppxPackage | Where-Object Name -match 'HEIF|HEVC|Raw|Webp' | ForEach-Object { $_.Name + ' ' + $_.Version }"))
    lire_par_windows()
    a = Appli(pw)
    try:
        a.deposer(JEU / "formats")
        resultat, mur, _ = a.analyser()
        R.note(f"analyse : {resultat['photos']} photos en {resultat['dureeMs'] / 1000:.1f} s")
        nd = resultat["nonDecodees"]
        attendu = {"heif": non_lus("formats/", {".heic", ".heif"}), "raw": non_lus("formats/", RAW), "autres": non_lus("formats/", {".webp"})}
        R.ok(nd == attendu, "Jumelles décode les HEIC, RAW et WebP que Windows décode", f"{nd} pour {attendu}")
        groupes, fichiers = verifier_groupes(resultat, "formats/", cle_jeu, "Formats")
        # RAW et JPEG de la meme prise : taille vue par Windows, taille du capteur, photo gardee
        import rawpy
        for g in groupes:
            for k, m in g:
                if Path(k).suffix.lower() in RAW:
                    with rawpy.imread(win(m["chemin"])) as raw:
                        capteur = f"{raw.sizes.width} x {raw.sizes.height}"
                    gardee = "le RAW" if k == g[0][0] else f"{g[0][0].split('/')[-1]} ({g[0][1]['largeur']} x {g[0][1]['hauteur']})"
                    R.note(f"{k.split('/')[-1]} : {m['largeur']} x {m['hauteur']} pour Windows, capteur {capteur}, gardée : {gardee}")
        R.capture(a.page, "15-formats-ensemble")
        # chaque groupe s'affiche, vignettes comprises
        echecs = []
        for i in range(len(groupes)):
            a.ev("i => { etat.onglet = 'tous'; aller(i) }", i)
            dims = a.vignettes(".photo img", timeout=120)
            noms = a.ev("liste()[etat.courant].membres.map(m => m.chemin.split('\\\\').pop())")
            if any(w == 0 for w, _ in dims):
                echecs.append(f"{noms} : {dims}")
            if i < 3 or any(n.lower().endswith((".heic", ".cr3", ".nef")) for n in noms):
                R.capture(a.page, f"16-formats-groupe-{i + 1}")
        R.ok(not echecs, "vignettes des HEIC, RAW et WebP", " | ".join(echecs[:4]))
    finally:
        a.fermer()


def main():
    # WebView2 ignore WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS et HKCU quand l'appli tourne en
    # administrateur, comme tout ce que lance le runner : la strategie machine reste lue
    # (WebView2Feedback #5645). Le port DevTools est donne par Appli.
    ps("New-Item -Force 'HKLM:\\SOFTWARE\\Policies\\Microsoft\\Edge\\WebView2\\AdditionalBrowserArguments' | Out-Null")
    R.note(ps("(Get-CimInstance Win32_OperatingSystem).Caption + ' ' + (Get-CimInstance Win32_OperatingSystem).Version"))
    R.note(f"processeurs logiques : {os.cpu_count()}")
    lire_par_windows()
    with sync_playwright() as pw:
        for scenario in (
            lambda: scenario_principal(pw),
            lambda: scenario_reseau(pw),
            lambda: scenario_amovible(pw),
            lambda: scenario_cache(pw),
            lambda: scenario_anglais(pw),
            lambda: scenario_volume(pw),
            lambda: EXTENSIONS and scenario_extensions(pw),
        ):
            try:
                scenario()
            except Exception:
                R.ok(False, "le scénario va jusqu'au bout", traceback.format_exc().strip().splitlines()[-1])
                R.bloc(traceback.format_exc())
    R.ecrire()
    print(f"\n{len(R.echecs)} échec(s)")
    sys.exit(1 if R.echecs else 0)


if __name__ == "__main__":
    main()
