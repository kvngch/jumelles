"""Jeu de photos de la recette et groupes attendus.

python recette/photos.py <telechargements> <jeu>

Les originaux viennent de Wikimedia Commons (commons.tsv) et de raw.pixls.us (RAW
ci-dessous, CC0). <telechargements> les garde d'un passage a l'autre. <jeu> recoit un
dossier par scenario et attendu.json, qui donne pour chaque fichier la scene dont il
vient et sa variante. Deux fichiers d'une meme scene doivent finir dans le meme groupe,
deux scenes differentes jamais.
"""

import hashlib
import json
import os
import random
import shutil
import sys
import time
import urllib.parse
import urllib.request
from concurrent.futures import ProcessPoolExecutor, ThreadPoolExecutor
from pathlib import Path

import pillow_heif
import rawpy
from PIL import Image, ImageDraw, ImageEnhance, ImageOps

pillow_heif.register_heif_opener()

ICI = Path(__file__).parent
UA = {"User-Agent": "jumelles-recette (https://github.com/kvngch/jumelles)"}

# (nom sur raw.pixls.us, numero, sha256)
RAW = [
    ("Canon - EOS 5D Mark III.CR2", "771", "ec069b178b9383d80c72f8cdc1b79bd54ca451e7c20793b109d8ad73bef6bdf6"),
    ("Canon - EOS R6 - 3:2.CR3", "4659", "74abb0a113d075ad9887a058082f40dd2a938c4813a08474d82356f11a027778"),
    (
        "Nikon - D750 - 12bit 12bit compressed (Lossy (type 2)) (3:2).NEF",
        "895",
        "e4a3f85e014275fc0a47600a373af95ec850872a5f6cf87649bcaa42b77ef2e4",
    ),
    (
        "Sony - ILCE-7M3 - 12bit 12bit compressed (3:2).ARW",
        "2415",
        "3ae2ff75cfa7240d0b4458f662afd28665c5fd1e83d230ac73f5ad69d3df8592",
    ),
    ("Apple - iPhone 6s Plus - 16bit (4:3).DNG", "915", "845fee4f09c832af3728778737f5ae9b99124debb821b6e2066b94aed1daa91c"),
    ("Olympus - E-M10 Mark IIIs - 16bit (4:3).ORF", "4927", "8bc0009eaacdbeacc72606581665702a1242bc6d4b405d1f0b3f149fe6c85e55"),
    ("Panasonic - DC-G90 - 4:3.RW2", "4151", "41de770701618b7276871fd1566da08782bf1009977c5bea91c0cc300bb8ed65"),
    (
        "Fujifilm - X-T30 II - 14bit 14bit compressed (3:2).RAF",
        "5064",
        "5e2678038377cc7f146ceb7693c08535fa1af162cec705d06b0446c3d3017d47",
    ),
    ("Pentax - K-1 Mark II - 14bit (3:2).PEF", "7133", "fe2330254c5313d4dde97cbd01e2987f6f1137f1c9c687d30e951f4668979381"),
    (
        "Samsung - NX500 - 12bit 12bit normal compression (3:2).SRW",
        "3668",
        "156e43118811bcecb6fcc600b5f41ee1773c4f00b6a5b2dd72e177e35f6596ab",
    ),
]

# Retouches du jeu principal, deux par original de 20 a 79
RETOUCHES = ["gris", "eclaircie", "miroir", "tournee", "orientation", "recadree", "png", "tiff", "bmp", "gif"]

# 2020-01-01 : les originaux sont les plus anciens, leurs copies et variantes viennent apres
T0 = 1577836800


def telecharger(url, cible, empreinte, algo):
    if cible.exists() and hashlib.new(algo, cible.read_bytes()).hexdigest() == empreinte:
        return
    for essai in range(5):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=300) as r:
                data = r.read()
            if hashlib.new(algo, data).hexdigest() != empreinte:
                raise ValueError(f"empreinte differente : {url}")
            cible.write_bytes(data)
            return
        except Exception as e:
            print(f"essai {essai + 1} : {url} : {e}", flush=True)
            time.sleep(10 * (essai + 1))
    raise RuntimeError(f"echec du telechargement : {url}")


def originaux(dossier):
    """Chemins des originaux Commons, puis des RAW, telecharges s'ils manquent"""
    dossier.mkdir(parents=True, exist_ok=True)
    lignes = [l.split("\t") for l in (ICI / "commons.tsv").read_text().splitlines() if not l.startswith("#")]
    taches = [(url.strip(), dossier / f"{sha1}.jpg", sha1, "sha1") for sha1, _, _, _, url in lignes]
    for nom, numero, sha256 in RAW:
        url = f"https://raw.pixls.us/getfile.php/{numero}/nice/{urllib.parse.quote(nom)}"
        taches.append((url, dossier / f"{sha256[:16]}.{nom.rsplit('.', 1)[1]}", sha256, "sha256"))
    with ThreadPoolExecutor(4) as pool:
        list(pool.map(lambda t: telecharger(*t), taches))
    commons = [t[1] for t in taches[: len(lignes)]]
    raws = [(nom.rsplit(".", 1)[0], t[1]) for (nom, _, _), t in zip(RAW, taches[len(lignes) :])]
    return commons, raws


def ouvrir(src, cote=None):
    im = Image.open(src)
    if cote:
        im.draft("RGB", (cote, cote))
    im = ImageOps.exif_transpose(im).convert("RGB")
    if cote:
        im.thumbnail((cote, cote), Image.LANCZOS)
    return im


def variante(tache):
    """Ecrit une variante d'une photo, cote = taille de la variante"""
    src, dst, sorte = tache
    dst = Path(dst)
    dst.parent.mkdir(parents=True, exist_ok=True)
    if sorte == "copie":
        shutil.copyfile(src, dst)
    elif sorte == "reduite":
        ouvrir(src, 1600).save(dst, quality=75)
    elif sorte == "gris":
        ouvrir(src).convert("L").save(dst, quality=90)
    elif sorte == "eclaircie":
        ImageEnhance.Brightness(ouvrir(src)).enhance(1.15).save(dst, quality=90)
    elif sorte == "miroir":
        ImageOps.mirror(ouvrir(src)).save(dst, quality=90)
    elif sorte == "tournee":
        ouvrir(src).transpose(Image.Transpose.ROTATE_270).save(dst, quality=90)
    elif sorte == "orientation":
        # pixels tournes d'un quart de tour a gauche, etiquette EXIF 6 qui les remet droits
        exif = Image.Exif()
        exif[0x0112] = 6
        ouvrir(src).transpose(Image.Transpose.ROTATE_90).save(dst, quality=90, exif=exif.tobytes())
    elif sorte == "recadree":
        im = ouvrir(src)
        dx, dy = im.width * 3 // 100, im.height * 3 // 100
        im.crop((dx, dy, im.width - dx, im.height - dy)).save(dst, quality=90)
    elif sorte == "png":
        ouvrir(src, 2000).save(dst)
    elif sorte == "tiff":
        ouvrir(src, 2000).save(dst, compression="tiff_lzw")
    elif sorte == "bmp":
        ouvrir(src, 1600).save(dst)
    elif sorte == "gif":
        ouvrir(src, 1200).convert("P", palette=Image.Palette.ADAPTIVE).save(dst)
    elif sorte == "webp":
        ouvrir(src, 2000).save(dst, quality=80)
    elif sorte == "heic":
        ouvrir(src, 3000).save(dst, quality=80)
    elif sorte == "apercu":
        with rawpy.imread(str(src)) as raw:
            vignette = raw.extract_thumb()
        if vignette.format == rawpy.ThumbFormat.JPEG:
            dst.write_bytes(vignette.data)
        else:
            Image.fromarray(vignette.data).save(dst, quality=90)
    elif sorte == "volume":
        # image de synthese : fond en degrade et formes au hasard, propres a la scene
        r = random.Random(int(src))
        im = Image.linear_gradient("L").resize((640, 480)).convert("RGB")
        im = Image.merge("RGB", [b.point(lambda v, k=r.randint(1, 3): v * k % 256) for b in im.split()])
        d = ImageDraw.Draw(im)
        for _ in range(14):
            x, y = r.randint(-100, 600), r.randint(-100, 440)
            boite = (x, y, x + r.randint(40, 300), y + r.randint(40, 300))
            couleur = tuple(r.randint(0, 255) for _ in range(3))
            (d.ellipse if r.random() < 0.5 else d.rectangle)(boite, fill=couleur)
        im.save(dst, quality=85)
    elif sorte == "volume-reduite":
        Image.open(src).resize((320, 240), Image.LANCZOS).save(dst, quality=80)
    else:
        raise ValueError(sorte)
    return str(dst)


class Jeu:
    def __init__(self, racine):
        self.racine = racine
        self.attendu = {}
        self.taches = []
        self.dates = {}

    def ajouter(self, src, rel, sorte, scene, jours, etiquette=None):
        self.taches.append((str(src), long(self.racine / rel), sorte))
        self.attendu[rel] = {"scene": scene, "variante": etiquette or sorte}
        self.dates[rel] = T0 + jours * 86400 + len(self.dates) * 60

    def ecrire(self, rel, contenu, variante, scene=None):
        dst = self.racine / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        dst.write_bytes(contenu)
        self.attendu[rel] = {"scene": scene, "variante": variante}
        self.dates[rel] = T0 + 700 * 86400


def construire(commons, raws, racine):
    jeu = Jeu(racine)
    nom = lambda i: f"IMG_{1000 + i}"
    for i, src in enumerate(commons):
        jeu.ajouter(src, f"photos/Originaux/{nom(i)}.JPG", "copie", f"c{i}", i % 50, "original")
    for i in range(30):
        jeu.ajouter(commons[i], f"photos/Sauvegarde/{nom(i)}.JPG", "copie", f"c{i}", 400)
    for i in range(10):
        jeu.ajouter(commons[i], f"photos/Sauvegarde/2023/{nom(i)}.jpeg", "copie", f"c{i}", 500)
    for i in range(60):
        jeu.ajouter(commons[i], f"photos/WhatsApp/IMG-20230{i % 9 + 1}15-WA00{i:02}.jpg", "reduite", f"c{i}", 410)
    extensions = {"png": "png", "tiff": "tif", "bmp": "bmp", "gif": "gif"}
    for i in range(20, 80):
        for sorte in (RETOUCHES[i % 10], RETOUCHES[(i + 3) % 10]):
            ext = extensions.get(sorte, "jpg")
            jeu.ajouter(commons[i], f"photos/Retouches/{nom(i)}-{sorte}.{ext}", sorte, f"c{i}", 420)
    for i in range(80, 85):
        jeu.ajouter(commons[i], f"photos/iPhone/{nom(i)}.HEIC", "heic", f"c{i}", 430)
    for i in range(85, 90):
        jeu.ajouter(commons[i], f"photos/Web/{nom(i)}.webp", "webp", f"c{i}", 430)
    for k, (appareil, src) in enumerate(raws):
        ext = src.suffix.upper()
        jeu.ajouter(src, f"photos/RAW/{appareil}{ext}", "copie", f"r{k}", 10, "raw")
        jeu.ajouter(src, f"photos/RAW/JPEG/{appareil}.jpg", "apercu", f"r{k}", 440)

    # Cas limites
    jeu.ecrire("photos/Divers/vide.jpg", b"", "vide")
    jeu.ecrire("photos/Divers/texte.jpg", b"Ceci n'est pas une photo.\n" * 40, "texte")
    jeu.ecrire("photos/Divers/notes.txt", b"pas une photo", "ignore")
    tronquee = commons[104].read_bytes()
    jeu.ecrire("photos/Divers/tronquee.jpg", tronquee[: len(tronquee) * 2 // 5], "tronquee", "c104")
    jeu.ajouter(commons[100], "photos/Divers/Été à Montréal, 東京 😀.jpg", "copie", "c100", 450)
    jeu.ajouter(commons[101], "photos/Divers/A&B #1 100%.jpg", "copie", "c101", 450)
    profond = "/".join(["Un dossier au nom assez long pour"] * 8)
    jeu.ajouter(commons[102], f"photos/Divers/{profond}/{nom(102)}.JPG", "copie", "c102", 450, "chemin-long")

    # Jeux des autres scenarios : partage reseau (par lettre et par chemin UNC), support
    # amovible, interface en anglais, formats a extension, volume
    for dossier, i in (("reseau/lettre", 110), ("amovible", 113)):
        base = nom(i + 890)
        jeu.ajouter(commons[i], f"{dossier}/Vacances/{base}.JPG", "copie", f"c{i}", 20, "original")
        jeu.ajouter(commons[i], f"{dossier}/Vacances/Copie/{base}.JPG", "copie", f"c{i}", 400)
        jeu.ajouter(commons[i], f"{dossier}/Telephone/{base}.JPG", "reduite", f"c{i}", 410)
        jeu.ajouter(commons[i + 1], f"{dossier}/Vacances/{nom(i + 891)}.JPG", "copie", f"c{i + 1}", 21, "original")
        jeu.ajouter(commons[i + 1], f"{dossier}/Sauvegarde/{nom(i + 891)}.JPG", "copie", f"c{i + 1}", 400)
        jeu.ajouter(commons[i + 2], f"{dossier}/Vacances/{nom(i + 892)}.JPG", "copie", f"c{i + 2}", 22, "original")
        jeu.ajouter(commons[i + 2], f"{dossier}/Retouches/{nom(i + 892)}-nb.jpg", "gris", f"c{i + 2}", 420)
    jeu.ajouter(commons[116], f"reseau/unc/{nom(116)}.JPG", "copie", "c116", 20, "original")
    jeu.ajouter(commons[116], f"reseau/unc/Copie/{nom(116)}.JPG", "copie", "c116", 400)
    jeu.ajouter(commons[116], f"reseau/unc/{nom(116)}-miroir.jpg", "miroir", "c116", 420)
    for i, sorte in ((117, "copie"), (118, "gris")):
        jeu.ajouter(commons[i], f"anglais/{nom(i)}.JPG", "copie", f"c{i}", 20, "original")
        jeu.ajouter(commons[i], f"anglais/Export/{nom(i)}-{sorte}.jpg", sorte, f"c{i}", 400)
    jeu.ajouter(commons[119], f"anglais/{nom(119)}.HEIC", "heic", "c119", 400)
    for i in range(80, 90):
        jeu.ajouter(commons[i], f"formats/{nom(i)}.JPG", "copie", f"c{i}", 20, "original")
        ext, sorte = ("HEIC", "heic") if i < 85 else ("webp", "webp")
        jeu.ajouter(commons[i], f"formats/{ext}/{nom(i)}.{ext}", sorte, f"c{i}", 430)
    for k, (appareil, src) in enumerate(raws):
        jeu.ajouter(src, f"formats/RAW/{appareil}{src.suffix.upper()}", "copie", f"r{k}", 10, "raw")
        jeu.ajouter(src, f"formats/RAW/JPEG/{appareil}.jpg", "apercu", f"r{k}", 440)

    # Volume : 3 000 scenes de trois fichiers (original, copie, reduite) et 1 000 seules
    for n in range(4000):
        d = f"volume/{n // 500:02}"
        jeu.ajouter(n, f"{d}/V{n:05}.jpg", "volume", f"v{n}", 30, "original")
    volume = []
    for n in range(3000):
        d = f"volume/{n // 500:02}"
        volume.append((racine / f"{d}/V{n:05}.jpg", f"volume/copies/{n // 500:02}/V{n:05}.jpg", "copie", f"v{n}"))
        volume.append((racine / f"{d}/V{n:05}.jpg", f"volume/reduites/{n // 500:02}/V{n:05}-r.jpg", "volume-reduite", f"v{n}"))

    with ProcessPoolExecutor() as pool:
        list(pool.map(variante, jeu.taches, chunksize=8))
        jeu.taches = []
        for src, rel, sorte, scene in volume:
            jeu.ajouter(src, rel, sorte, scene, 400)
        list(pool.map(variante, jeu.taches, chunksize=32))

    # lien dur : un deuxieme chemin vers le meme fichier, qui ne doit pas compter comme copie
    os.link(racine / f"photos/Originaux/{nom(103)}.JPG", racine / "photos/Divers/lien-dur.jpg")
    jeu.attendu["photos/Divers/lien-dur.jpg"] = {"scene": "c103", "variante": "lien-dur"}

    for rel, t in jeu.dates.items():
        os.utime(long(racine / rel), (t, t))
    (racine / "attendu.json").write_text(json.dumps(jeu.attendu, ensure_ascii=False, indent=0), encoding="utf-8")
    return jeu.attendu


def long(chemin):
    """Chemin utilisable au-dela de 260 caracteres sous Windows"""
    p = str(Path(chemin).resolve())
    return "\\\\?\\" + p if os.name == "nt" and len(p) > 240 else p


if __name__ == "__main__":
    debut = time.time()
    commons, raws = originaux(Path(sys.argv[1]))
    print(f"originaux : {len(commons)} photos, {len(raws)} RAW, {time.time() - debut:.0f} s", flush=True)
    racine = Path(sys.argv[2]).resolve()
    shutil.rmtree(racine, ignore_errors=True)
    attendu = construire(commons, raws, racine)
    print(f"jeu : {len(attendu)} fichiers en {time.time() - debut:.0f} s")
