# Jumelles

Jumelles retrouve les photos en double ou similaires sous Windows : copies identiques, versions réduites ou recompressées, passées en noir et blanc, tournées ou en miroir. Un écran de tri montre chaque groupe côte à côte et envoie les photos choisies à la corbeille.

[English below](#english)

## Télécharger

Le zip de chaque version est sur la page des [versions](https://github.com/kvngch/jumelles/releases). Il contient un dossier avec `Jumelles.exe`, à lancer tel quel. Rien ne s'installe. Windows 10 ou 11, processeur x64.

L'exe n'est pas signé. Au premier lancement de chaque version, Windows affiche en général « Windows a protégé votre ordinateur » : cliquer sur « Informations complémentaires », puis sur « Exécuter quand même ». Cocher « Débloquer » dans les propriétés du zip avant de l'extraire devrait éviter cet écran. Sur un PC où Smart App Control est actif, Windows bloque l'exe sans proposer de le lancer.

Chaque version publie l'empreinte SHA-256 de son zip. Pour la comparer, dans PowerShell : `Get-FileHash jumelles-v<version>-windows-x64.zip`.

## Utilisation

1. Ajouter un ou plusieurs dossiers, par le bouton ou en les déposant dans la fenêtre. Les sous-dossiers sont parcourus.
2. Choisir la ressemblance (stricte, normale ou large), puis lancer l'analyse.
3. La vue d'ensemble liste les groupes. Dans chacun, la photo de plus grande définition, puis la plus ancienne, est gardée d'avance et les autres sont marquées à supprimer.
4. Ouvrir un groupe pour comparer ses photos et changer les marques. Au clavier, les flèches passent d'un groupe à l'autre, un chiffre ne garde que la photo correspondante, Espace agrandit et Échap revient à la liste.
5. « Envoyer à la corbeille » demande confirmation. Les photos restent récupérables dans la corbeille de Windows tant qu'elle n'est pas vidée. Les lecteurs réseau et les supports amovibles n'ont pas de corbeille : « Déplacer vers… » range leurs photos dans un dossier.

Un groupe garde toujours au moins une photo. Une photo modifiée depuis l'analyse n'est pas touchée.

## Formats

Windows lit le JPEG, le PNG, le GIF, le BMP, le TIFF et le WebP. Le HEIC des iPhone demande les extensions « Extensions d'image HEIF » et « Extensions vidéo HEVC » du Microsoft Store. Les RAW (CR2, CR3, NEF, ARW, DNG, ORF, RW2, RAF, PEF, SRW) demandent l'extension « Raw Image Extension ». Après une analyse, Jumelles indique combien de photos n'ont pas pu être lues et quelle extension installer.

## Fonctionnement

Les copies exactes sont regroupées par taille, puis par empreinte de leur contenu. Pour les photos proches, Jumelles réduit chaque photo à 32 × 32 pixels en niveaux de gris et calcule une empreinte de ses formes. Deux photos dont les empreintes diffèrent de peu sont regroupées. Une photo recadrée n'est en général pas reconnue.

Les empreintes sont gardées dans `jumelles.cache`, à côté de l'exe, ou dans `%LOCALAPPDATA%\Jumelles` si ce dossier n'accepte pas l'écriture. Une nouvelle analyse ne relit que les photos nouvelles ou modifiées. Supprimer ce fichier vide le cache.

## Compiler

Avec Rust stable, sous Windows 10 ou 11 :

```
cargo build --release -p jumelles
```

L'exe est `target\release\jumelles.exe`. Le moteur, dans `core`, se compile et se teste aussi sous Linux :

```
cargo test -p jumelles-core
```

La recette de bout en bout se lance à la demande dans GitHub Actions (workflow `Recette`, dossier `recette`). L'exe de release y est piloté sur un runner Windows, avec des photos de Wikimedia Commons et de raw.pixls.us et les extensions d'image du Microsoft Store. Son rapport et ses captures sont gardés en artefact.

## Licence

GPL-3.0 ou ultérieure, texte complet dans [LICENSE](LICENSE).

---

## English

Jumelles finds duplicate and similar photos on Windows: identical copies, resized or recompressed versions, black and white versions, rotated or mirrored photos. A sorting screen shows each group side by side and moves the chosen photos to the Recycle Bin.

### Download

Each version's zip is on the [releases](https://github.com/kvngch/jumelles/releases) page. It holds a folder with `Jumelles.exe`, which runs as is. Nothing gets installed. Windows 10 or 11, x64 processor.

The exe is not signed. On the first launch of each version, Windows usually shows "Windows protected your PC": click "More info", then "Run anyway". Ticking "Unblock" in the zip's properties before extracting it should avoid that screen. On a PC where Smart App Control is on, Windows blocks the exe without offering to run it.

Each version publishes the SHA-256 hash of its zip. To compare it, in PowerShell: `Get-FileHash jumelles-v<version>-windows-x64.zip`.

### Use

1. Add one or more folders, with the button or by dropping them on the window. Subfolders are scanned.
2. Pick the similarity (strict, normal or loose), then start the scan.
3. The overview lists the groups. In each one, the photo with the highest resolution, then the oldest, is kept by default and the others are marked for deletion.
4. Open a group to compare its photos and change the marks. On the keyboard, the arrows move between groups, a digit keeps only the matching photo, Space enlarges and Esc goes back to the list.
5. "Move to Recycle Bin" asks for confirmation. The photos can be restored from the Windows Recycle Bin until it is emptied. Network drives and removable media have no Recycle Bin: "Move to…" puts their photos in a folder.

A group always keeps at least one photo. A photo changed since the scan is left untouched.

### Formats

Windows reads JPEG, PNG, GIF, BMP, TIFF and WebP. iPhone HEIC needs the "HEIF Image Extensions" and "HEVC Video Extensions" from the Microsoft Store. RAW files (CR2, CR3, NEF, ARW, DNG, ORF, RW2, RAF, PEF, SRW) need the "Raw Image Extension". After a scan, Jumelles tells how many photos could not be read and which extension to install.

### How it works

Exact copies are grouped by size, then by a hash of their content. For similar photos, Jumelles shrinks each photo to 32 × 32 grayscale pixels and computes a fingerprint of its shapes. Two photos whose fingerprints differ slightly are grouped. A cropped photo is usually not recognised.

Fingerprints are kept in `jumelles.cache`, next to the exe, or in `%LOCALAPPDATA%\Jumelles` when that folder is read-only. A new scan only reads new or changed photos. Deleting the file empties the cache.

### Build

With stable Rust, on Windows 10 or 11:

```
cargo build --release -p jumelles
```

The exe is `target\release\jumelles.exe`. The engine, in `core`, also builds and runs its tests on Linux:

```
cargo test -p jumelles-core
```

The end-to-end acceptance test runs on demand in GitHub Actions (`Recette` workflow, `recette` folder). It drives the release exe on a Windows runner, with photos from Wikimedia Commons and raw.pixls.us and the Microsoft Store image extensions. Its report and screenshots are kept as an artifact.

### License

GPL-3.0 or later, full text in [LICENSE](LICENSE).
