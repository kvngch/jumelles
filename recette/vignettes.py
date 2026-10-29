"""Compare les vignettes du moteur (WIC) aux pixels des fichiers lus par Pillow, pillow-heif et
rawpy : luminance moyenne, contraste et ecart moyen, en niveaux de gris sur 256 pixels.

python recette/vignettes.py <dossier des photos> <dossier des vignettes>
"""

import sys
from pathlib import Path

import pillow_heif
import rawpy
from PIL import Image, ImageChops, ImageOps, ImageStat

pillow_heif.register_heif_opener()
RAW = {".cr2", ".cr3", ".nef", ".arw", ".dng", ".orf", ".rw2", ".raf", ".pef", ".srw"}


def gris(im):
    im = ImageOps.exif_transpose(im).convert("L")
    im.thumbnail((256, 256))
    return im


photos, vignettes = Path(sys.argv[1]), Path(sys.argv[2])
print(f"{'fichier':48} {'moy. fichier':>12} {'moy. moteur':>12} {'contr. fichier':>14} {'contr. moteur':>14} {'ecart':>6}")
for v in sorted(vignettes.glob("*.jpg")):
    source = next(photos.rglob(v.stem), None)
    if source is None:
        continue
    if source.suffix.lower() in RAW:
        with rawpy.imread(str(source)) as raw:
            ref = gris(Image.fromarray(raw.postprocess(use_camera_wb=True, half_size=True)))
    else:
        ref = gris(Image.open(source))
    mot = Image.open(v).convert("L").resize(ref.size)
    a, b = ImageStat.Stat(ref), ImageStat.Stat(mot)
    ecart = ImageStat.Stat(ImageChops.difference(ref, mot)).mean[0]
    print(f"{source.name[:48]:48} {a.mean[0]:12.1f} {b.mean[0]:12.1f} {a.stddev[0]:14.1f} {b.stddev[0]:14.1f} {ecart:6.1f}")
