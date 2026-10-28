//! Empreintes DCT (pHash) d'une image de 32 x 32 pixels en niveaux de gris : une
//! grossiere de 64 bits pour trouver les paires candidates, une fine de 256 bits pour
//! les confirmer.

use rayon::prelude::*;
use std::f32::consts::PI;
use std::sync::OnceLock;

pub const SIDE: usize = 32;

/// Frequences retenues dans chaque sens pour l'empreinte fine
const FINE: usize = 16;

/// Empreintes des huit orientations de l'image, l'originale en premier
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hashes {
    pub coarse: [u64; 8],
    pub fine: [[u64; 4]; 8],
}

/// cos(pi * (2x + 1) * f / 2N) pour les frequences f de 1 a FINE
fn cosines() -> &'static [[f32; SIDE]; FINE] {
    static T: OnceLock<[[f32; SIDE]; FINE]> = OnceLock::new();
    T.get_or_init(|| {
        std::array::from_fn(|u| {
            std::array::from_fn(|x| (PI * (2 * x + 1) as f32 * (u + 1) as f32 / (2 * SIDE) as f32).cos())
        })
    })
}

/// Coefficients DCT des frequences 1 a FINE dans chaque sens, composante continue
/// exclue, en c[v][u] avec u horizontal et v vertical
fn low_frequencies(px: &[u8; SIDE * SIDE]) -> [[f32; FINE]; FINE] {
    let t = cosines();
    let rows: [[f32; FINE]; SIDE] =
        std::array::from_fn(|y| std::array::from_fn(|u| (0..SIDE).map(|x| px[y * SIDE + x] as f32 * t[u][x]).sum()));
    std::array::from_fn(|v| std::array::from_fn(|u| (0..SIDE).map(|y| rows[y][u] * t[v][y]).sum()))
}

/// Bloc K x K des basses frequences dans l'orientation k. Un miroir horizontal change le
/// signe des frequences horizontales impaires, un miroir vertical celui des frequences
/// verticales impaires, une transposition echange les deux axes : leurs huit
/// combinaisons donnent les miroirs, le demi-tour et les quarts de tour.
fn oriented<const K: usize>(c: &[[f32; FINE]; FINE], k: usize) -> [[f32; K]; K] {
    let (transpose, flip_u, flip_v) = (k & 4 != 0, k & 1 != 0, k & 2 != 0);
    std::array::from_fn(|v| {
        std::array::from_fn(|u| {
            let x = if transpose { c[u][v] } else { c[v][u] };
            // indice 0 = frequence 1 : les frequences impaires ont un indice pair
            if (flip_u && u % 2 == 0) ^ (flip_v && v % 2 == 0) {
                -x
            } else {
                x
            }
        })
    })
}

/// Un bit par coefficient, a 1 au-dessus de la mediane
fn bits<const W: usize>(values: &[f32]) -> [u64; W] {
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(f32::total_cmp);
    let median = (sorted[values.len() / 2 - 1] + sorted[values.len() / 2]) / 2.0;
    let mut h = [0u64; W];
    for (i, &v) in values.iter().enumerate() {
        if v > median {
            h[i / 64] |= 1 << (i % 64);
        }
    }
    h
}

pub fn hashes(px: &[u8; SIDE * SIDE]) -> Hashes {
    let c = low_frequencies(px);
    Hashes {
        coarse: std::array::from_fn(|k| bits::<1>(oriented::<8>(&c, k).as_flattened())[0]),
        fine: std::array::from_fn(|k| bits::<4>(oriented::<FINE>(&c, k).as_flattened())),
    }
}

/// Distance fine, en bits sur 256, entre `a` et l'orientation de `b` la plus proche, parmi
/// celles dont les deux empreintes restent sous le seuil `max` exprime en bits sur 64
pub fn near(a: &Hashes, b: &Hashes, max: u32) -> Option<u32> {
    (0..8)
        .filter(|&k| (a.coarse[0] ^ b.coarse[k]).count_ones() <= max)
        .map(|k| {
            a.fine[0]
                .iter()
                .zip(&b.fine[k])
                .map(|(x, y)| (x ^ y).count_ones())
                .sum()
        })
        .filter(|&d| d <= 4 * max)
        .min()
}

/// Pour chaque empreinte, les suivantes qui en sont proches, avec leur distance fine.
// ponytail: toutes les paires comparees, O(n^2), quelques secondes a 100 000 photos ;
// un index (multi-index hashing) si des bibliotheques plus grandes le demandent
pub fn close_pairs(hashes: &[Hashes], max: u32) -> Vec<Vec<(usize, u32)>> {
    // Empreintes grossieres a part et contigues : le test rapide ecarte presque toutes les
    // paires sans charger les empreintes fines
    let coarse: Vec<[u64; 8]> = hashes.iter().map(|h| h.coarse).collect();
    (0..hashes.len())
        .into_par_iter()
        .map(|a| {
            let mut candidates = Vec::new();
            coarse_row(&coarse, a, max, &mut candidates);
            candidates
                .into_iter()
                .filter_map(|b| near(&hashes[a], &hashes[b], max).map(|d| (b, d)))
                .collect()
        })
        .collect()
}

/// Indices b > a dont une orientation passe le seuil grossier. L'instruction popcnt, que
/// tous les processeurs x86-64 ont depuis 2008, compte les bits plusieurs fois plus vite
/// que la suite d'operations du jeu d'instructions de base.
fn coarse_row(coarse: &[[u64; 8]], a: usize, max: u32, out: &mut Vec<usize>) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("popcnt") {
        // SAFETY: popcnt vient d'etre detecte sur ce processeur
        return unsafe { coarse_row_popcnt(coarse, a, max, out) };
    }
    coarse_row_body(coarse, a, max, out)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "popcnt")]
fn coarse_row_popcnt(coarse: &[[u64; 8]], a: usize, max: u32, out: &mut Vec<usize>) {
    coarse_row_body(coarse, a, max, out)
}

#[inline(always)]
fn coarse_row_body(coarse: &[[u64; 8]], a: usize, max: u32, out: &mut Vec<usize>) {
    let c = coarse[a][0];
    for (b, hb) in coarse.iter().enumerate().skip(a + 1) {
        let mut best = 64;
        for &h in hb {
            best = best.min((c ^ h).count_ones());
        }
        if best <= max {
            out.push(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Px = [u8; SIDE * SIDE];

    /// Image sans symetrie, au spectre riche comme une photo : ondulations, tache dans un
    /// coin et bruit pseudo-aleatoire
    fn image() -> Px {
        let mut seed = 12345u32;
        std::array::from_fn(|i| {
            let (x, y) = ((i % SIDE) as f32, (i / SIDE) as f32);
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let mut v = 110.0 + 50.0 * (0.31 * x + 0.17 * y).sin() + 35.0 * (0.03 * x * y).cos();
            if x < 10.0 && y < 8.0 {
                v += 40.0;
            }
            (v + (seed >> 24) as f32 / 8.0).clamp(0.0, 255.0) as u8
        })
    }

    fn rot90(p: &Px) -> Px {
        std::array::from_fn(|i| {
            let (x, y) = (i % SIDE, i / SIDE);
            p[(SIDE - 1 - x) * SIDE + y]
        })
    }

    fn mirror(p: &Px) -> Px {
        std::array::from_fn(|i| {
            let (x, y) = (i % SIDE, i / SIDE);
            p[y * SIDE + SIDE - 1 - x]
        })
    }

    /// Plus petite distance, grossiere et fine, entre `a` et les huit orientations de `b`
    fn closest(a: &Hashes, b: &Hashes) -> (u32, u32) {
        (0..8)
            .map(|k| {
                let fine = a.fine[0]
                    .iter()
                    .zip(&b.fine[k])
                    .map(|(x, y)| (x ^ y).count_ones())
                    .sum();
                ((a.coarse[0] ^ b.coarse[k]).count_ones(), fine)
            })
            .min()
            .unwrap()
    }

    #[test]
    fn huit_orientations() {
        let a = image();
        let ha = hashes(&a);
        let mut p = a;
        for _ in 0..4 {
            for q in [p, mirror(&p)] {
                let (coarse, fine) = closest(&hashes(&q), &ha);
                assert!(coarse <= 2 && fine <= 8, "orientation non reconnue : {coarse}, {fine}");
            }
            p = rot90(&p);
        }
        // un quart de tour retrouve l'original par les orientations, pas sans elles
        let hr = hashes(&rot90(&a));
        assert!(near(&hr, &ha, 10).is_some_and(|d| d <= 8));
        assert!((hr.coarse[0] ^ ha.coarse[0]).count_ones() > 16);
    }

    #[test]
    fn images_differentes() {
        let a = image();
        let b: Px = std::array::from_fn(|i| ((i % SIDE) * 7 % 256) as u8 ^ ((i / SIDE) * 5) as u8);
        assert!(near(&hashes(&a), &hashes(&b), 16).is_none());
        assert_eq!(near(&hashes(&a), &hashes(&a), 0), Some(0));
    }
}
