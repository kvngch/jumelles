const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { open } = window.__TAURI__.dialog;
const { getCurrentWebview } = window.__TAURI__.webview;

const $ = (id) => document.getElementById(id);
const nombre = new Intl.NumberFormat('fr-FR');
const jour = new Intl.DateTimeFormat('fr-FR', { dateStyle: 'medium' });
const dossiers = [];
let enCours = false;

// En francais, 0 et 1 prennent le singulier
const accord = (n, un, plusieurs) => `${nombre.format(n)} ${n > 1 ? plusieurs : un}`;
// Guillemets francais, espaces fines insecables a l'interieur
const cite = (s) => `\u00AB\u202F${s}\u202F\u00BB`;

function poids(octets) {
  const unites = ['o', 'Ko', 'Mo', 'Go', 'To'];
  let i = 0;
  while (octets >= 1024 && i < unites.length - 1) {
    octets /= 1024;
    i++;
  }
  return `${new Intl.NumberFormat('fr-FR', { maximumFractionDigits: i ? 1 : 0 }).format(octets)} ${unites[i]}`;
}

function element(nom, texte, classe) {
  const e = document.createElement(nom);
  if (texte !== undefined) e.textContent = texte;
  if (classe) e.className = classe;
  return e;
}

function afficherDossiers() {
  $('liste').replaceChildren(...dossiers.map((d, i) => {
    const retirer = element('button', 'Retirer');
    retirer.type = 'button';
    retirer.onclick = () => {
      dossiers.splice(i, 1);
      afficherDossiers();
    };
    const li = element('li');
    li.append(element('span', d, 'chemin'), retirer);
    return li;
  }));
  $('vide').hidden = dossiers.length > 0;
  $('analyser').disabled = enCours || dossiers.length === 0;
}

function ajouter(chemins) {
  for (const c of chemins) if (!dossiers.includes(c)) dossiers.push(c);
  afficherDossiers();
}

function ressemblance(f, i) {
  if (i === 0) return 'référence';
  if (f.identique) return 'identique';
  return `ressemblance ${Math.round(100 - (f.ecart * 100) / 256)} %`;
}

function afficherResultats(r) {
  const liberable = r.groupes.reduce((s, g) => s + g.slice(1).reduce((t, f) => t + f.taille, 0), 0);
  let etat = accord(r.photos, 'photo analysée', 'photos analysées');
  if (r.illisibles) etat += `, ${accord(r.illisibles, 'fichier ou dossier illisible', 'fichiers ou dossiers illisibles')}`;
  $('etat').textContent = etat;
  const n = r.nonDecodees;
  const avis = [];
  if (n.heif) {
    avis.push(`${accord(n.heif, "photo HEIC n'a pas pu être lue", "photos HEIC n'ont pas pu être lues")}. Windows lit ce format avec les extensions ${cite("Extensions d'image HEIF")} et ${cite('Extensions vidéo HEVC')} du Microsoft Store.`);
  }
  if (n.raw) {
    avis.push(`${accord(n.raw, "photo RAW n'a pas pu être lue", "photos RAW n'ont pas pu être lues")}. Windows lit ce format avec l'extension ${cite('Raw Image Extension')} du Microsoft Store.`);
  }
  if (n.autres) {
    avis.push(`${accord(n.autres, "photo n'a pas pu être lue", "photos n'ont pas pu être lues")} : fichier abîmé ou format non pris en charge.`);
  }
  $('avis').replaceChildren(...avis.map((a) => element('li', a)));
  $('titre-resultats').textContent = r.groupes.length
    ? `${accord(r.groupes.length, 'groupe', 'groupes')}, jusqu'à ${poids(liberable)} à libérer`
    : 'Aucune photo en double';
  $('groupes').replaceChildren(...r.groupes.map((g) => {
    const identiques = g.every((f) => f.identique);
    const ul = element('ul');
    ul.append(...g.map((f, i) => {
      const li = element('li');
      li.append(
        element('span', f.chemin, 'chemin'),
        element('span', f.largeur ? `${f.largeur} × ${f.hauteur}` : ''),
        element('span', `${poids(f.taille)}, ${jour.format(new Date(f.modifie))}`),
        element('span', ressemblance(f, i)),
      );
      return li;
    }));
    const article = element('article');
    article.append(element('h3', identiques ? `${g.length} copies identiques` : `${g.length} photos proches`), ul);
    return article;
  }));
  $('resultats').hidden = false;
}

$('ajouter').onclick = async () => {
  const choix = await open({ directory: true, multiple: true });
  if (choix) ajouter([choix].flat());
};

getCurrentWebview().onDragDropEvent((e) => {
  if (e.payload.type === 'drop' && !enCours) ajouter(e.payload.paths);
});

listen('avancement', (e) => {
  if (!enCours) return;
  const a = e.payload;
  let etat = accord(a.trouvees, 'photo trouvée', 'photos trouvées');
  if (a.aDecoder) etat = `Empreintes : ${nombre.format(a.decodees)} sur ${accord(a.aDecoder, 'photo', 'photos')}`;
  else if (a.aLire) etat = `Copies exactes : ${nombre.format(a.lues)} sur ${accord(a.aLire, 'fichier lu', 'fichiers lus')}`;
  $('etat').textContent = etat;
});

$('analyser').onclick = async () => {
  enCours = true;
  $('ajouter').disabled = true;
  $('resultats').hidden = true;
  $('etat').textContent = 'Analyse en cours';
  afficherDossiers();
  try {
    const r = await invoke('analyser', { dossiers, ecart: Number($('ecart').value) });
    enCours = false;
    afficherResultats(r);
  } catch (err) {
    enCours = false;
    $('etat').textContent = `Échec de l'analyse : ${err}`;
  }
  $('ajouter').disabled = false;
  afficherDossiers();
};

afficherDossiers();
