// Interface texts in English. f.n formats a number, f.p picks singular or plural by the
// language rules, f.poids formats a file size, f.cite quotes.
window.TEXTES = window.TEXTES || {};
window.TEXTES.en = {
  code: 'en',
  cite: (s) => `\u201C${s}\u201D`,
  unites: ['B', 'KB', 'MB', 'GB', 'TB'],
  duree: (f, ms) =>
    ms < 60000 ? `${f.n(Math.max(1, Math.round(ms / 1000)))} s` : `${Math.floor(ms / 60000)} min ${Math.round((ms % 60000) / 1000)} s`,

  dossiers: 'Folders to scan',
  aucunDossier: 'No folder yet. Add one or drop it on the window.',
  ajouter: 'Add a folder',
  retirer: 'Remove',
  ressemblance: 'Similarity',
  niveaux: ['Strict', 'Normal', 'Loose'],
  analyser: 'Scan',
  analyseEnCours: 'Scanning',
  trouvees: (f, n) => `${f.n(n)} ${f.p(n, 'photo found', 'photos found')}`,
  copiesExactes: (f, fait, total) => `Exact copies: ${f.n(fait)} of ${f.n(total)} ${f.p(total, 'file read', 'files read')}`,
  empreintes: (f, fait, total) => `Fingerprints: ${f.n(fait)} of ${f.n(total)} ${f.p(total, 'photo', 'photos')}`,
  echec: (e) => `Scan failed: ${e}`,

  nouvelleAnalyse: 'New scan',
  nPhotos: (f, n) => `${f.n(n)} ${f.p(n, 'photo', 'photos')}`,
  groupes: 'Groups',
  sousTitre: (f, groupes, photos, duree) =>
    `${f.n(groupes)} ${f.p(groupes, 'group', 'groups')} among ${f.n(photos)} ${f.p(photos, 'photo', 'photos')}, scanned in ${duree}. The framed photo is the one that stays: the RAW file if there is one, otherwise the highest resolution, then the oldest.`,
  aucunGroupe: (f, photos, duree) =>
    `No duplicate among ${f.n(photos)} ${f.p(photos, 'photo', 'photos')}, scanned in ${duree}.`,
  illisibles: (f, n) => `${f.n(n)} unreadable ${f.p(n, 'file or folder', 'files or folders')} skipped.`,
  heif: (f, n) =>
    `${f.n(n)} HEIC ${f.p(n, 'photo could', 'photos could')} not be read. Windows reads this format with the ${f.cite('HEIF Image Extensions')} and ${f.cite('HEVC Video Extensions')} from the Microsoft Store.`,
  raw: (f, n) =>
    `${f.n(n)} RAW ${f.p(n, 'photo could', 'photos could')} not be read. Windows reads this format with the ${f.cite('Raw Image Extension')} from the Microsoft Store.`,
  autres: (f, n) => `${f.n(n)} ${f.p(n, 'photo could', 'photos could')} not be read: damaged file or unsupported format.`,
  onglets: { tous: 'All', identiques: 'Identical', proches: 'Similar', aVerifier: 'To review' },
  colonnes: ['Photos', 'Similarity and folders', 'Photos', 'To free', 'Status'],
  identiques: 'Identical',
  proches: 'Similar',
  pourcent: (f, x) => `${f.n(x)}%`,
  etats: { pret: 'Ready', aVerifier: 'To review', verifie: 'Reviewed' },
  aucunDansOnglet: 'No group in this tab.',

  tousLesGroupes: '← All groups',
  groupeNumero: (f, i, n) => `Group ${f.n(i)} of ${f.n(n)}`,
  sousTitreIdentiques: 'Identical copies, byte for byte',
  sousTitreProches: (f, x) => `Similar photos, at least ${f.n(x)}% alike the reference photo`,
  precedent: 'Previous',
  suivant: 'Next',
  gardee: 'Kept',
  aSupprimer: 'To delete',
  garder: 'Keep',
  supprimer: 'Delete',
  proprietes: ['Dimensions', 'Size', 'Date', 'Folder', 'Name', 'Similarity'],
  reference: 'Reference',
  identique: 'Identical',
  plusGrande: 'largest',
  plusAncienne: 'oldest',
  raccourcis: ['previous or next group', 'keep this photo', 'enlarge', 'all groups'],
  echap: 'Esc',
  espace: 'Space',
  auMoinsUne: 'A group keeps at least one photo.',
  agrandie: 'Enlarged photo',
  fermer: 'Close',

  barre: (f, n, octets) => `<b>${f.n(n)} ${f.p(n, 'photo', 'photos')}</b> to delete, <b>${f.poids(octets)}</b> to free`,
  aucuneMarquee: 'No photo marked for deletion',
  deplacerVers: 'Move to…',
  corbeille: 'Move to Recycle Bin',
  confirmerTitre: (f, n) => `Move ${f.n(n)} ${f.p(n, 'photo', 'photos')} to the Recycle Bin?`,
  confirmerTexte: (f, octets) =>
    `Space freed: ${f.poids(octets)}. The photos can be restored from the Windows Recycle Bin until it is emptied.`,
  annuler: 'Cancel',
  envoyees: (f, n) => `${f.n(n)} ${f.p(n, 'photo', 'photos')} moved to the Recycle Bin.`,
  deplacees: (f, n, d) => `${f.n(n)} ${f.p(n, 'photo', 'photos')} moved to ${d}.`,
  sansCorbeille: (f, n) =>
    `${f.n(n)} ${f.p(n, 'photo is', 'photos are')} on a drive without a Recycle Bin, network or removable. They stay marked: choose a folder to move them to.`,
  modifiees: (f, n) => `${f.n(n)} ${f.p(n, 'photo has', 'photos have')} changed since the scan and ${f.p(n, 'was', 'were')} left untouched.`,
  restees: (f, n) =>
    `${f.n(n)} ${f.p(n, 'photo', 'photos')} could not be removed: file open in another program, or deletion declined.`,
  refus: 'Request refused: a group would lose all its photos.',
  echecSuppression: (e) => `Failed: ${e}`,
};
