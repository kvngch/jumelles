// Textes de l'interface en francais. f.n formate un nombre, f.p choisit le singulier ou
// le pluriel selon les regles de la langue (0 et 1 au singulier en francais), f.poids
// formate une taille de fichier, f.cite met entre guillemets.
window.TEXTES = window.TEXTES || {};
window.TEXTES.fr = {
  code: 'fr',
  cite: (s) => `\u00AB\u202F${s}\u202F\u00BB`,
  unites: ['o', 'Ko', 'Mo', 'Go', 'To'],
  duree: (f, ms) =>
    ms < 60000 ? `${f.n(Math.max(1, Math.round(ms / 1000)))} s` : `${Math.floor(ms / 60000)} min ${Math.round((ms % 60000) / 1000)} s`,

  dossiers: 'Dossiers à analyser',
  aucunDossier: 'Aucun dossier. Ajoutez-en un ou déposez-le dans la fenêtre.',
  ajouter: 'Ajouter un dossier',
  retirer: 'Retirer',
  ressemblance: 'Ressemblance',
  niveaux: ['Stricte', 'Normale', 'Large'],
  analyser: 'Analyser',
  analyseEnCours: 'Analyse en cours',
  trouvees: (f, n) => `${f.n(n)} ${f.p(n, 'photo trouvée', 'photos trouvées')}`,
  copiesExactes: (f, fait, total) => `Copies exactes : ${f.n(fait)} sur ${f.n(total)} ${f.p(total, 'fichier lu', 'fichiers lus')}`,
  empreintes: (f, fait, total) => `Empreintes : ${f.n(fait)} sur ${f.n(total)} ${f.p(total, 'photo', 'photos')}`,
  echec: (e) => `Échec de l'analyse : ${e}`,

  nouvelleAnalyse: 'Nouvelle analyse',
  nPhotos: (f, n) => `${f.n(n)} ${f.p(n, 'photo', 'photos')}`,
  groupes: 'Groupes',
  sousTitre: (f, groupes, photos, duree) =>
    `${f.n(groupes)} ${f.p(groupes, 'groupe', 'groupes')} parmi ${f.n(photos)} ${f.p(photos, 'photo analysée', 'photos analysées')} en ${duree}. La photo encadrée est celle qui reste : le RAW s'il y en a un, sinon la plus grande définition, puis la plus ancienne.`,
  aucunGroupe: (f, photos, duree) =>
    `Aucune photo en double parmi ${f.n(photos)} ${f.p(photos, 'photo analysée', 'photos analysées')} en ${duree}.`,
  illisibles: (f, n) => `${f.n(n)} ${f.p(n, 'fichier ou dossier illisible ignoré', 'fichiers ou dossiers illisibles ignorés')}.`,
  heif: (f, n) =>
    `${f.n(n)} ${f.p(n, "photo HEIC n'a pas pu être lue", "photos HEIC n'ont pas pu être lues")}. Windows lit ce format avec les extensions ${f.cite("Extensions d'image HEIF")} et ${f.cite('Extensions vidéo HEVC')} du Microsoft Store.`,
  raw: (f, n) =>
    `${f.n(n)} ${f.p(n, "photo RAW n'a pas pu être lue", "photos RAW n'ont pas pu être lues")}. Windows lit ce format avec l'extension ${f.cite('Raw Image Extension')} du Microsoft Store.`,
  autres: (f, n) =>
    `${f.n(n)} ${f.p(n, "photo n'a pas pu être lue", "photos n'ont pas pu être lues")} : fichier abîmé ou format non pris en charge.`,
  onglets: { tous: 'Tous', identiques: 'Identiques', proches: 'Proches', aVerifier: 'À vérifier' },
  colonnes: ['Photos', 'Ressemblance et dossiers', 'Photos', 'À libérer', 'État'],
  identiques: 'Identiques',
  proches: 'Proches',
  pourcent: (f, x) => `${f.n(x)}\u202F%`,
  etats: { pret: 'Prêt', aVerifier: 'À vérifier', verifie: 'Vérifié' },
  aucunDansOnglet: 'Aucun groupe dans cet onglet.',

  tousLesGroupes: '← Tous les groupes',
  groupeNumero: (f, i, n) => `Groupe ${f.n(i)} sur ${f.n(n)}`,
  sousTitreIdentiques: 'Copies identiques, octet pour octet',
  sousTitreProches: (f, x) => `Photos proches, au moins ${f.n(x)}\u202F% de ressemblance avec la photo de référence`,
  precedent: 'Précédent',
  suivant: 'Suivant',
  gardee: 'Gardée',
  aSupprimer: 'À supprimer',
  garder: 'Garder',
  supprimer: 'Supprimer',
  proprietes: ['Dimensions', 'Poids', 'Date', 'Dossier', 'Nom', 'Ressemblance'],
  reference: 'Référence',
  identique: 'Identique',
  plusGrande: 'plus grande',
  plusAncienne: 'plus ancienne',
  raccourcis: ['groupe précédent ou suivant', 'garder cette photo', 'agrandir', 'tous les groupes'],
  echap: 'Échap',
  espace: 'Espace',
  auMoinsUne: 'Un groupe garde au moins une photo.',
  agrandie: 'Photo agrandie',
  fermer: 'Fermer',

  barre: (f, n, octets) =>
    `<b>${f.n(n)} ${f.p(n, 'photo', 'photos')}</b> à supprimer, <b>${f.poids(octets)}</b> à libérer`,
  aucuneMarquee: 'Aucune photo marquée à supprimer',
  deplacerVers: 'Déplacer vers…',
  corbeille: 'Envoyer à la corbeille',
  confirmerTitre: (f, n) => `Envoyer ${f.n(n)} ${f.p(n, 'photo', 'photos')} à la corbeille ?`,
  confirmerTexte: (f, octets) =>
    `Place libérée : ${f.poids(octets)}. Les photos restent récupérables dans la corbeille de Windows tant qu'elle n'est pas vidée.`,
  annuler: 'Annuler',
  envoyees: (f, n) => `${f.n(n)} ${f.p(n, 'photo envoyée', 'photos envoyées')} à la corbeille.`,
  deplacees: (f, n, d) => `${f.n(n)} ${f.p(n, 'photo déplacée', 'photos déplacées')} dans ${d}.`,
  sansCorbeille: (f, n) =>
    `${f.n(n)} ${f.p(n, 'photo est', 'photos sont')} sur un lecteur sans corbeille, réseau ou support amovible. ${f.p(n, 'Elle reste marquée : choisissez un dossier où la déplacer.', 'Elles restent marquées : choisissez un dossier où les déplacer.')}`,
  modifiees: (f, n) =>
    `${f.n(n)} ${f.p(n, "photo a changé depuis l'analyse et n'a pas été touchée", "photos ont changé depuis l'analyse et n'ont pas été touchées")}.`,
  restees: (f, n) =>
    `${f.n(n)} ${f.p(n, "photo n'a pas pu partir", "photos n'ont pas pu partir")} : fichier ouvert dans un autre programme, ou suppression refusée.`,
  refus: 'Demande refusée : un groupe perdrait toutes ses photos.',
  echecSuppression: (e) => `Échec : ${e}`,
};
