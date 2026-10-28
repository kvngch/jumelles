//! Moteur de Jumelles : parcours des dossiers, empreintes, regroupement.

use std::path::Path;

/// Extensions retenues par le parcours, comparees sans tenir compte de la casse.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif",
    "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "raf", "pef", "srw",
];

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions() {
        assert!(is_image(Path::new("vacances/IMG_0001.JPG")));
        assert!(is_image(Path::new("a.heic")));
        assert!(is_image(Path::new("b.Cr3")));
        assert!(!is_image(Path::new("notes.txt")));
        assert!(!is_image(Path::new("jpg")));
        assert!(!is_image(Path::new("archive.jpg.zip")));
    }
}
