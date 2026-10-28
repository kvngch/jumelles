const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { open } = window.__TAURI__.dialog;
const { getCurrentWebview } = window.__TAURI__.webview;

// Langue de Windows : francais si elle l'est, anglais sinon
const LANGUE = navigator.language || 'en';
const T = LANGUE.toLowerCase().startsWith('fr') ? window.TEXTES.fr : window.TEXTES.en;
document.documentElement.lang = T.code;
const nombre = new Intl.NumberFormat(LANGUE);
const regles = new Intl.PluralRules(LANGUE);
const jour = new Intl.DateTimeFormat(LANGUE, { dateStyle: 'medium', timeStyle: 'short' });
const f = {
  n: (x) => nombre.format(x),
  p: (x, un, plusieurs) => (regles.select(x) === 'one' ? un : plusieurs),
  poids(octets) {
    let i = 0;
    while (octets >= 1024 && i < T.unites.length - 1) {
      octets /= 1024;
      i++;
    }
    return `${new Intl.NumberFormat(LANGUE, { maximumFractionDigits: i ? 1 : 0 }).format(octets)} ${T.unites[i]}`;
  },
  cite: T.cite,
};

const $ = (id) => document.getElementById(id);
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
const separateur = (c) => Math.max(c.lastIndexOf('\\'), c.lastIndexOf('/'));
const parent = (c) => c.slice(0, separateur(c));
const nom = (c) => c.slice(separateur(c) + 1);
// Coupure de ligne permise apres chaque separateur de chemin
const chemin = (c) => esc(c).replace(/[\\/]/g, '$&<wbr>');
// Trois derniers dossiers, assez pour reconnaitre l'endroit dans la vue d'ensemble
const dossierCourt = (c) => parent(c).split(/[\\/]/).slice(-3).join('\\');

// Sous Windows, WebView2 sert les protocoles internes a l'adresse http://<nom>.localhost
const BASE = navigator.userAgent.includes('Windows') ? 'http://vignette.localhost/' : 'vignette://localhost/';
const vignette = (id, cote) => `${BASE}${etat.numero}/${id}?t=${Math.round(cote * Math.min(2, devicePixelRatio || 1))}`;

const ICONES = {
  pret: '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="1.5" y="1.5" width="9" height="9" rx="1.5" fill="currentColor"/></svg>',
  aVerifier: '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><circle cx="6" cy="6" r="4.5" fill="none" stroke="var(--ambre)" stroke-width="1.6"/></svg>',
  verifie: '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M1.8 6.3 4.6 9 10.2 3" fill="none" stroke="currentColor" stroke-width="1.8"/></svg>',
};

const etat = {
  ecran: 'accueil', // accueil, groupes ou groupe
  dossiers: [],
  ecart: 10,
  enCours: false,
  statut: '',
  numero: 0,
  resultat: null,
  groupes: [], // { membres, vu }
  marques: new Set(), // photos a supprimer
  onglet: 'tous',
  courant: 0, // indice dans la liste de l'onglet
  avis: [], // bilan de la derniere suppression
  message: '',
  occupe: false,
};

const identiques = (g) => g.membres.every((m) => m.identique);
const ressemblance = (m) => Math.round(100 - (m.ecart * 100) / 256);
const plusFaible = (g) => Math.min(...g.membres.slice(1).map(ressemblance));
const etatGroupe = (g) => (identiques(g) ? 'pret' : g.vu ? 'verifie' : 'aVerifier');
const filtres = {
  tous: () => true,
  identiques: (g) => identiques(g),
  proches: (g) => !identiques(g),
  aVerifier: (g) => !identiques(g) && !g.vu,
};
const liste = () => etat.groupes.filter(filtres[etat.onglet]);
const aLiberer = (g) => g.membres.reduce((s, m) => s + (etat.marques.has(m.id) ? m.taille : 0), 0);
const groupeDe = (id) => etat.groupes.find((g) => g.membres.some((m) => m.id === id));
const resume = (g) => (identiques(g) ? T.identiques : `${T.proches}, ${T.pourcent(f, plusFaible(g))}`);

function charger(r) {
  etat.resultat = r;
  etat.numero = r.numero;
  etat.groupes = r.groupes.map((membres) => ({ membres, vu: false }));
  // la premiere photo de chaque groupe reste, les autres sont proposees a la suppression
  etat.marques = new Set(r.groupes.flatMap((g) => g.slice(1).map((m) => m.id)));
  etat.onglet = 'tous';
  etat.courant = 0;
  etat.avis = [];
}

function messagesAnalyse(r) {
  const m = [];
  if (r.illisibles) m.push(T.illisibles(f, r.illisibles));
  if (r.nonDecodees.heif) m.push(T.heif(f, r.nonDecodees.heif));
  if (r.nonDecodees.raw) m.push(T.raw(f, r.nonDecodees.raw));
  if (r.nonDecodees.autres) m.push(T.autres(f, r.nonDecodees.autres));
  return m;
}

function accueil() {
  const n = etat.dossiers.length;
  const dossiers = etat.dossiers
    .map((d, i) => `<li><span>${chemin(d)}</span><button type="button" class="bouton" data-retirer="${i}">${T.retirer}</button></li>`)
    .join('');
  const niveaux = [4, 10, 14]
    .map((v, i) => `<option value="${v}" ${v === etat.ecart ? 'selected' : ''}>${T.niveaux[i]}</option>`)
    .join('');
  return `<section class="accueil">
    <h1 class="titre">${T.dossiers}</h1>
    <ul class="dossiers">${dossiers}</ul>
    ${n ? '' : `<p class="discret">${T.aucunDossier}</p>`}
    <div class="actions">
      <button type="button" class="bouton" data-action="ajouter" ${etat.enCours ? 'disabled' : ''}>${T.ajouter}</button>
      <label class="reglage">${T.ressemblance}<select id="ecart">${niveaux}</select></label>
      <button type="button" class="bouton principal" data-action="analyser" ${etat.enCours || !n ? 'disabled' : ''}>${T.analyser}</button>
    </div>
    <p id="statut" class="discret" role="status">${esc(etat.statut)}</p>
  </section>`;
}

function ligne(g, i) {
  const visibles = g.membres.slice(0, 4);
  const plus = g.membres.length - visibles.length;
  const vignettes =
    visibles
      .map((m) => `<img src="${vignette(m.id, 52)}" class="${etat.marques.has(m.id) ? 'supprimee' : 'gardee'}" alt="" loading="lazy">`)
      .join('') + (plus > 0 ? `<span class="plus">+${plus}</span>` : '');
  const type = identiques(g) ? `<b>${T.identiques}</b>` : `<b>${T.proches}</b>, ${T.pourcent(f, plusFaible(g))}`;
  const lieux = [...new Set(g.membres.map((m) => dossierCourt(m.chemin)))].join(', ');
  const e = etatGroupe(g);
  return `<div class="ligne" tabindex="0" role="button" data-ouvrir="${i}">
    <div><div class="vignettes">${vignettes}</div></div>
    <div class="type"><span>${type}<small>${esc(lieux)}</small></span></div>
    <div class="droite num">${f.n(g.membres.length)}</div>
    <div class="droite num">${f.poids(aLiberer(g))}</div>
    <div class="etat">${ICONES[e]}<span>${T.etats[e]}</span></div>
  </div>`;
}

function ensemble() {
  const r = etat.resultat;
  const duree = T.duree(f, r.dureeMs);
  const avis = [...etat.avis, ...messagesAnalyse(r)].map((a) => `<li>${esc(a)}</li>`).join('');
  if (!etat.groupes.length) {
    return `<section class="ensemble">
      <h1 class="titre">${T.groupes}</h1>
      <p class="sous-titre discret">${T.aucunGroupe(f, r.photos, duree)}</p>
      <ul class="avis">${avis}</ul>
    </section>`;
  }
  const onglets = Object.keys(filtres)
    .map((k) => {
      const n = etat.groupes.filter(filtres[k]).length;
      return `<button type="button" class="onglet ${k === etat.onglet ? 'actif' : ''}" data-onglet="${k}">${T.onglets[k]}<span class="num">${f.n(n)}</span></button>`;
    })
    .join('');
  const lignes = liste().map(ligne).join('') || `<p class="discret vide">${T.aucunDansOnglet}</p>`;
  const colonnes = T.colonnes.map((c, i) => `<div class="${i === 2 || i === 3 ? 'droite' : ''}">${c}</div>`).join('');
  return `<section class="ensemble">
    <h1 class="titre">${T.groupes}</h1>
    <p class="sous-titre discret">${T.sousTitre(f, etat.groupes.length, r.photos, duree)}</p>
    <ul class="avis">${avis}</ul>
    <nav class="onglets">${onglets}</nav>
    <div class="tableau"><div class="entete">${colonnes}</div>${lignes}</div>
  </section>`;
}

function fiche(g, m, i) {
  const supprimee = etat.marques.has(m.id);
  const pixels = (x) => x.largeur * x.hauteur;
  const grande = new Set(g.membres.map(pixels)).size > 1 && pixels(m) === Math.max(...g.membres.map(pixels));
  const ancienne = new Set(g.membres.map((x) => x.modifie)).size > 1 && m.modifie === Math.min(...g.membres.map((x) => x.modifie));
  const res = i === 0 ? T.reference : m.identique ? T.identique : T.pourcent(f, ressemblance(m));
  const [dims, poids, date, dossier, nomFichier, ressemble] = T.proprietes;
  return `<article class="photo ${supprimee ? 'supprimee' : 'gardee'}" data-id="${m.id}">
    <button type="button" class="cadre" data-zoom="${m.id}" aria-label="${esc(nom(m.chemin))}"><img src="${vignette(m.id, 800)}" alt=""><span class="marque-photo">${supprimee ? T.aSupprimer : T.gardee}</span></button>
    <div class="choix" role="group" aria-label="${esc(nom(m.chemin))}">
      <button type="button" aria-pressed="${!supprimee}" data-garder="${m.id}">${T.garder}</button>
      <button type="button" aria-pressed="${supprimee}" data-supprimer="${m.id}">${T.supprimer}</button>
    </div>
    <dl>
      <dt>${dims}</dt><dd class="num">${m.largeur ? `${m.largeur} × ${m.hauteur}` : '?'}${grande ? ` <em>${T.plusGrande}</em>` : ''}</dd>
      <dt>${poids}</dt><dd class="num">${f.poids(m.taille)}</dd>
      <dt>${date}</dt><dd class="num"><span class="insecable">${jour.format(new Date(m.modifie))}</span>${ancienne ? ` <em>${T.plusAncienne}</em>` : ''}</dd>
      <dt>${dossier}</dt><dd>${chemin(parent(m.chemin))}</dd>
      <dt>${nomFichier}</dt><dd>${esc(nom(m.chemin))}</dd>
      <dt>${ressemble}</dt><dd class="num">${res}</dd>
    </dl>
  </article>`;
}

function page() {
  const l = liste();
  const g = l[etat.courant];
  g.vu = true;
  const menu = l
    .map(
      (h, i) =>
        `<li class="${i === etat.courant ? 'courant' : ''}" data-aller="${i}"><img src="${vignette(h.membres[0].id, 40)}" alt="" loading="lazy"><span>${resume(h)}<small class="num">${T.nPhotos(f, h.membres.length)}, ${f.poids(aLiberer(h))}</small></span></li>`,
    )
    .join('');
  const [suivantOuPrecedent, garderCelleCi, agrandir, tous] = T.raccourcis;
  const chiffres = g.membres
    .slice(0, 9)
    .map((_, i) => `<kbd>${i + 1}</kbd>`)
    .join('');
  return `<section class="groupe">
    <nav class="menu"><button type="button" class="retour" data-action="ensemble">${T.tousLesGroupes}</button><ol>${menu}</ol></nav>
    <div class="page">
      <div class="page-tete">
        <div>
          <h1 class="titre">${T.groupeNumero(f, etat.courant + 1, l.length)}</h1>
          <p class="sous-titre discret">${identiques(g) ? T.sousTitreIdentiques : T.sousTitreProches(f, plusFaible(g))}</p>
        </div>
        <button type="button" class="bouton" data-action="precedent" ${etat.courant === 0 ? 'disabled' : ''}>${T.precedent}</button>
        <button type="button" class="bouton" data-action="suivant" ${etat.courant >= l.length - 1 ? 'disabled' : ''}>${T.suivant}</button>
      </div>
      <p id="message" class="message" role="status">${esc(etat.message)}</p>
      <div class="photos n-${Math.min(g.membres.length, 4)}">${g.membres.map((m, i) => fiche(g, m, i)).join('')}</div>
      <p class="raccourcis">
        <span><kbd>←</kbd><kbd>→</kbd>${suivantOuPrecedent}</span>
        <span>${chiffres}${garderCelleCi}</span>
        <span><kbd>${T.espace}</kbd>${agrandir}</span>
        <span><kbd>${T.echap}</kbd>${tous}</span>
      </p>
    </div>
  </section>`;
}

function barre() {
  const visible = etat.ecran !== 'accueil' && etat.resultat && etat.groupes.length > 0;
  $('action').hidden = !visible;
  $('nouvelle').hidden = etat.ecran === 'accueil';
  $('resume').textContent = etat.ecran === 'accueil' ? '' : etat.dossiers.join(', ');
  if (!visible) return;
  const membres = etat.groupes.flatMap((g) => g.membres).filter((m) => etat.marques.has(m.id));
  const octets = membres.reduce((s, m) => s + m.taille, 0);
  $('total').innerHTML = membres.length ? T.barre(f, membres.length, octets) : esc(T.aucuneMarquee);
  $('corbeille').disabled = $('deplacer').disabled = !membres.length || etat.occupe;
}

function rendre() {
  if (etat.ecran === 'groupe' && !liste()[etat.courant]) etat.ecran = 'groupes';
  const ecrans = { accueil, groupes: ensemble, groupe: page };
  $('ecran').innerHTML = ecrans[etat.ecran]();
  if (etat.ecran === 'accueil') $('ecart').onchange = (e) => (etat.ecart = Number(e.target.value));
  barre();
}

// Change les marques d'une page de groupe sur place, sans recharger ses vignettes
function majMarques() {
  for (const a of document.querySelectorAll('.photo')) {
    const supprimee = etat.marques.has(Number(a.dataset.id));
    a.classList.toggle('supprimee', supprimee);
    a.classList.toggle('gardee', !supprimee);
    a.querySelector('.marque-photo').textContent = supprimee ? T.aSupprimer : T.gardee;
    a.querySelector('[data-garder]').setAttribute('aria-pressed', !supprimee);
    a.querySelector('[data-supprimer]').setAttribute('aria-pressed', supprimee);
  }
  const m = $('message');
  if (m) m.textContent = etat.message;
  const courant = document.querySelector('.menu li.courant small');
  const g = liste()[etat.courant];
  if (courant && g) courant.textContent = `${T.nPhotos(f, g.membres.length)}, ${f.poids(aLiberer(g))}`;
  barre();
}

function marquer(id, supprimer) {
  const g = groupeDe(id);
  if (supprimer && !g.membres.some((m) => m.id !== id && !etat.marques.has(m.id))) {
    etat.message = T.auMoinsUne;
  } else {
    etat.message = '';
    if (supprimer) etat.marques.add(id);
    else etat.marques.delete(id);
  }
  majMarques();
}

function garderSeulement(k) {
  const g = liste()[etat.courant];
  const choisie = g?.membres[k];
  if (!choisie) return;
  for (const m of g.membres) {
    if (m.id === choisie.id) etat.marques.delete(m.id);
    else etat.marques.add(m.id);
  }
  etat.message = '';
  majMarques();
}

function aller(i) {
  const l = liste();
  if (i < 0 || i >= l.length) return;
  etat.courant = i;
  etat.message = '';
  etat.ecran = 'groupe';
  rendre();
  document.querySelector('.page')?.scrollTo(0, 0);
  document.querySelector('.menu li.courant')?.scrollIntoView({ block: 'nearest' });
}

function zoom(id) {
  $('zoom-image').src = vignette(id, 2000);
  $('zoom-image').dataset.id = id;
  $('zoom').hidden = false;
  $('zoom-fermer').focus();
}

function fermerZoom() {
  $('zoom').hidden = true;
  $('zoom-image').removeAttribute('src');
}

function ajouter(chemins) {
  for (const c of chemins) if (!etat.dossiers.includes(c)) etat.dossiers.push(c);
  rendre();
}

async function analyser() {
  etat.enCours = true;
  etat.statut = T.analyseEnCours;
  rendre();
  try {
    charger(await invoke('analyser', { dossiers: etat.dossiers, ecart: etat.ecart }));
    etat.ecran = 'groupes';
    etat.statut = '';
  } catch (err) {
    etat.statut = T.echec(err);
  }
  etat.enCours = false;
  rendre();
}

// Retire de l'affichage les photos parties et ecrit le bilan
function appliquer(b, dossier) {
  const parties = new Set(b.parties);
  for (const g of etat.groupes) g.membres = g.membres.filter((m) => !parties.has(m.id));
  etat.groupes = etat.groupes.filter((g) => g.membres.length > 1);
  const restantes = new Set(etat.groupes.flatMap((g) => g.membres.map((m) => m.id)));
  // une photo modifiee depuis l'analyse n'est plus proposee : il faut une nouvelle analyse
  for (const id of [...etat.marques]) if (!restantes.has(id) || b.modifiees.includes(id)) etat.marques.delete(id);
  etat.avis = [];
  if (b.parties.length) etat.avis.push(dossier ? T.deplacees(f, b.parties.length, dossier) : T.envoyees(f, b.parties.length));
  if (b.sansCorbeille.length) etat.avis.push(T.sansCorbeille(f, b.sansCorbeille.length));
  if (b.modifiees.length) etat.avis.push(T.modifiees(f, b.modifiees.length));
  if (b.restees.length) etat.avis.push(T.restees(f, b.restees.length));
}

async function traiter(commande, args) {
  etat.occupe = true;
  barre();
  try {
    appliquer(await invoke(commande, args), args.dossier);
  } catch (err) {
    etat.avis = [String(err).startsWith('refus') ? T.refus : T.echecSuppression(err)];
  }
  etat.occupe = false;
  etat.courant = 0;
  etat.ecran = 'groupes';
  rendre();
}

function confirmerCorbeille() {
  const membres = etat.groupes.flatMap((g) => g.membres).filter((m) => etat.marques.has(m.id));
  if (!membres.length) return;
  $('confirmer-titre').textContent = T.confirmerTitre(f, membres.length);
  $('confirmer-texte').textContent = T.confirmerTexte(f, membres.reduce((s, m) => s + m.taille, 0));
  $('confirmer').showModal();
  $('confirmer-non').focus();
}

async function deplacer() {
  const dossier = await open({ directory: true, multiple: false });
  if (dossier) await traiter('deplacer', { ids: [...etat.marques], dossier });
}

const actions = {
  accueil: () => {
    etat.ecran = 'accueil';
    rendre();
  },
  ajouter: async () => {
    const choix = await open({ directory: true, multiple: true });
    if (choix) ajouter([choix].flat());
  },
  analyser,
  ensemble: () => {
    etat.ecran = 'groupes';
    rendre();
  },
  precedent: () => aller(etat.courant - 1),
  suivant: () => aller(etat.courant + 1),
  corbeille: confirmerCorbeille,
  deplacer,
};

document.addEventListener('click', (e) => {
  const b = e.target.closest('[data-action],[data-retirer],[data-onglet],[data-ouvrir],[data-aller],[data-garder],[data-supprimer],[data-zoom]');
  if (!b || b.disabled) return;
  const d = b.dataset;
  if (d.action) actions[d.action]();
  else if (d.retirer) {
    etat.dossiers.splice(Number(d.retirer), 1);
    rendre();
  } else if (d.onglet) {
    etat.onglet = d.onglet;
    rendre();
  } else if (d.ouvrir) aller(Number(d.ouvrir));
  else if (d.aller) aller(Number(d.aller));
  else if (d.garder) marquer(Number(d.garder), false);
  else if (d.supprimer) marquer(Number(d.supprimer), true);
  else if (d.zoom) zoom(Number(d.zoom));
});

document.addEventListener('keydown', (e) => {
  if ($('confirmer').open) return;
  if (!$('zoom').hidden) {
    if (e.key === 'Escape') fermerZoom();
    return;
  }
  const surControle = e.target.closest('button, select, input');
  if (etat.ecran === 'groupe') {
    if (e.key === 'ArrowLeft') aller(etat.courant - 1);
    else if (e.key === 'ArrowRight') aller(etat.courant + 1);
    else if (e.key === 'Escape') actions.ensemble();
    else if (/^[1-9]$/.test(e.key)) garderSeulement(Number(e.key) - 1);
    else if (e.key === ' ' && !surControle) {
      const g = liste()[etat.courant];
      zoom((g.membres.find((m) => !etat.marques.has(m.id)) || g.membres[0]).id);
    } else return;
    e.preventDefault();
  } else if (etat.ecran === 'groupes') {
    const lignes = [...document.querySelectorAll('.ligne')];
    const i = lignes.indexOf(e.target);
    if (e.key === 'Enter' && i >= 0) aller(Number(e.target.dataset.ouvrir));
    else if (e.key === 'ArrowDown') lignes[Math.min(i + 1, lignes.length - 1)]?.focus();
    else if (e.key === 'ArrowUp') lignes[Math.max(i - 1, 0)]?.focus();
    else return;
    e.preventDefault();
  }
});

$('zoom').addEventListener('click', (e) => {
  if (e.target !== $('zoom-image')) fermerZoom();
});
// Double-clic : la photo s'ouvre dans la visionneuse de Windows
$('zoom-image').addEventListener('dblclick', () => invoke('ouvrir', { id: Number($('zoom-image').dataset.id) }));
$('zoom-image').alt = T.agrandie;
$('zoom-fermer').textContent = T.fermer;
$('confirmer-non').textContent = T.annuler;
$('confirmer-oui').textContent = T.corbeille;
$('confirmer-non').onclick = () => $('confirmer').close();
$('confirmer-oui').onclick = () => {
  $('confirmer').close();
  traiter('supprimer', { ids: [...etat.marques] });
};
$('nouvelle').textContent = T.nouvelleAnalyse;
$('deplacer').textContent = T.deplacerVers;
$('corbeille').textContent = T.corbeille;

getCurrentWebview().onDragDropEvent((e) => {
  if (e.payload.type === 'drop' && etat.ecran === 'accueil' && !etat.enCours) ajouter(e.payload.paths);
});

listen('avancement', (e) => {
  if (!etat.enCours) return;
  const a = e.payload;
  etat.statut = a.aDecoder
    ? T.empreintes(f, a.decodees, a.aDecoder)
    : a.aLire
      ? T.copiesExactes(f, a.lues, a.aLire)
      : T.trouvees(f, a.trouvees);
  const s = $('statut');
  if (s) s.textContent = etat.statut;
});

rendre();
