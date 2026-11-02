//! Parcours de la médiathèque.

use super::model::*;
use super::parse;
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const VIDEO_EXT: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "wmv", "ts", "m2ts", "mts", "mpg", "mpeg", "vob", "webm",
    "flv", "ogm", "divx", "iso",
];

/// Fichiers qui accompagnent une vidéo et la suivent lors du renommage.
pub const SIDECAR_EXT: &[&str] = &[
    "srt", "ass", "ssa", "sub", "idx", "sup", "vtt", "nfo", "jpg", "jpeg", "png", "tbn", "txt",
];

static CATEGORY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(films?|movies?|cin[eé]ma|s[eé]ries?( ?tv)?|tv( ?shows?)?|shows?|dessins? anim[eé]s?|animes?|anim[eé]s?|documentaires?|docs?|concerts?|spectacles?|dvd|blu-?ray|bd|uhd|4k|hd|remux|kids?|enfants?|jeunesse)$").unwrap()
});
static SKIP_DIR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(extras?|bonus|featurettes?|behind the scenes|deleted scenes|interviews?|trailers?|samples?|@eadir|\.\w.*|#recycle|lost\+found|\$recycle\.bin)$").unwrap()
});
static SERIES_CAT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)s[eé]ries?|tv|shows?|animes?").unwrap());

fn ext_lower(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_video(p: &Path) -> bool {
    VIDEO_EXT.contains(&ext_lower(p).as_str())
}

/// Dossier disque : contient VIDEO_TS ou BDMV.
fn disc_layout(dir: &Path) -> Option<Layout> {
    if dir.join("BDMV").is_dir() || dir.join("bdmv").is_dir() {
        Some(Layout::BlurayFolder)
    } else if dir.join("VIDEO_TS").is_dir() || dir.join("video_ts").is_dir() {
        Some(Layout::DvdFolder)
    } else {
        None
    }
}

/// Sous-dossiers qui font partie de la structure d'un disque : jamais renommés, jamais parcourus.
const DISC_STRUCT: &[&str] = &["bdmv", "video_ts", "audio_ts", "certificate", "aacs", "bdsvm", "jar", "makemkv", "hvdvd_ts"];

pub fn is_disc_struct(name: &str) -> bool {
    DISC_STRUCT.contains(&name.to_lowercase().as_str())
}

#[derive(Debug, PartialEq)]
pub enum Disc {
    /// Dossier qui ne contient que la structure d'un disque (+ éventuels .nfo, jaquettes…)
    Clean(Layout),
    /// Structure de disque mélangée à d'autres films / dossiers : on ne peut pas renommer le dossier
    Mixed(Layout),
}

pub fn disc_check(dir: &Path) -> Option<Disc> {
    let layout = disc_layout(dir)?;
    let Ok(rd) = std::fs::read_dir(dir) else { return Some(Disc::Clean(layout)) };
    let mixed = rd.flatten().any(|e| {
        let p = e.path();
        let n = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            !is_disc_struct(&n) && !SKIP_DIR.is_match(&n)
        } else {
            is_video(&p)
        }
    });
    Some(if mixed { Disc::Mixed(layout) } else { Disc::Clean(layout) })
}

/// Dossier « catégorie » : le plus profond des ancêtres nommés Films / Séries / UHD…, sinon la racine.
pub fn category_for(path: &Path, root: &Path) -> PathBuf {
    let mut best = root.to_path_buf();
    for a in path.ancestors().skip(1) {
        if !a.starts_with(root) || a == root {
            break;
        }
        if let Some(n) = a.file_name() {
            if CATEGORY.is_match(&n.to_string_lossy()) {
                best = a.to_path_buf();
                break; // le plus proche du média = le plus profond
            }
        }
    }
    best
}

pub fn scan(root: &Path) -> Vec<MediaItem> {
    let mut items = vec![];
    let mut walker = WalkDir::new(root).follow_links(true).sort_by_file_name().into_iter();
    while let Some(entry) = walker.next() {
        let Ok(entry) = entry else { continue };
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type().is_dir() {
            if p != root && (SKIP_DIR.is_match(&name) || is_disc_struct(&name)) {
                walker.skip_current_dir();
                continue;
            }
            match disc_check(p) {
                Some(Disc::Clean(layout)) => {
                    let mut it = make_item(p.to_path_buf(), layout, root);
                    if p == root {
                        it.locked = Some("C'est le dossier ouvert lui-même : ouvrez plutôt son dossier parent pour pouvoir le renommer.".into());
                    } else if p == it.category_dir || CATEGORY.is_match(&name) {
                        it.locked = Some(format!("La structure du disque est posée directement dans « {name} », qui est un dossier de rangement : il ne sera pas renommé. Placez BDMV / VIDEO_TS dans un dossier dédié au film."));
                    }
                    items.push(it);
                    walker.skip_current_dir();
                }
                Some(Disc::Mixed(layout)) => {
                    // Disque « orphelin » à côté d'autres médias : signalé, et on continue
                    // à parcourir les autres sous-dossiers normalement.
                    let mut it = make_item(p.to_path_buf(), layout, root);
                    it.locked = Some(format!("Ce dossier contient un {} mais aussi d'autres films ou dossiers : le renommer toucherait tout son contenu. Placez la structure du disque dans un dossier dédié.", if layout == Layout::DvdFolder { "VIDEO_TS" } else { "BDMV" }));
                    items.push(it);
                }
                None => {}
            }
            continue;
        }
        if !entry.file_type().is_file() || !is_video(p) {
            continue;
        }
        let lname = name.to_lowercase();
        let size = entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
        if lname.contains("sample") && size < 400 * 1024 * 1024 {
            continue;
        }
        if lname.contains("trailer") || lname.contains("-featurette") {
            continue;
        }
        let layout = if ext_lower(p) == "iso" { Layout::Iso } else { Layout::File };
        items.push(make_item(p.to_path_buf(), layout, root));
    }
    items
}

pub fn make_item(path: PathBuf, layout: Layout, root: &Path) -> MediaItem {
    let category_dir = category_for(&path, root);
    let guess = parse::guess_for(&path, layout.is_dir(), &category_dir);
    let in_series_cat = category_dir
        .file_name()
        .map(|n| SERIES_CAT.is_match(&n.to_string_lossy()))
        .unwrap_or(false);
    let kind = if guess.is_episode() && (guess.season.is_some() || in_series_cat) {
        MediaKind::Episode
    } else {
        MediaKind::Movie
    };
    let mut tech = TechInfo::default();
    super::probe::fill_from_tags(&mut tech, &guess.tags);
    MediaItem {
        path,
        layout,
        kind,
        category_dir,
        guess,
        tech,
        matched: None,
        candidates: vec![],
        episode_title: None,
        status: Status::Pending,
        included: true,
        message: None,
        locked: None,
    }
}

/// Fichiers annexes d'une vidéo : même dossier, nom commençant par le même radical.
pub fn sidecars(video: &Path) -> Vec<PathBuf> {
    let Some(dir) = video.parent() else { return vec![] };
    let Some(stem) = video.file_stem().map(|s| s.to_string_lossy().to_string()) else {
        return vec![];
    };
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p != video && p.is_file())
        .filter(|p| SIDECAR_EXT.contains(&ext_lower(p).as_str()))
        .filter(|p| {
            p.file_name()
                .map(|n| {
                    let n = n.to_string_lossy();
                    n.starts_with(&format!("{stem}.")) || n.starts_with(&format!("{stem}-"))
                })
                .unwrap_or(false)
        })
        .collect()
}

/// Nombre de vidéos principales dans un dossier (récursif, hors bonus ; un disque compte pour 1).
pub fn count_videos(dir: &Path) -> usize {
    if matches!(disc_check(dir), Some(Disc::Clean(_))) {
        return 1;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_lowercase();
        if p.is_dir() {
            if !SKIP_DIR.is_match(&name) && !is_disc_struct(&name) {
                n += count_videos(&p);
            }
        } else if is_video(&p) && !name.contains("sample") {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn disc_structures() {
        let t = std::env::temp_dir().join(format!("ohtpc-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&t);
        touch(&t.join("Films/Heat (1995)/DISC1/BDMV/STREAM/00800.m2ts"));
        touch(&t.join("Films/Heat (1995)/DISC2/BDMV/STREAM/00001.m2ts"));
        touch(&t.join("Films/Inception/BDMV/STREAM/00001.m2ts"));
        fs::create_dir_all(t.join("Films/Inception/CERTIFICATE")).unwrap();
        touch(&t.join("Films/BDMV/STREAM/00002.m2ts")); // disque égaré dans une catégorie
        touch(&t.join("Films/Amelie/VIDEO_TS/VTS_01_1.VOB"));

        let items = scan(&t);
        let find = |rel: &str| items.iter().find(|i| i.path == t.join(rel));
        assert_eq!(items.len(), 5, "{:#?}", items.iter().map(|i| &i.path).collect::<Vec<_>>());
        assert!(find("Films").unwrap().locked.is_some());
        assert!(find("Films/Heat (1995)/DISC1").unwrap().locked.is_none());
        assert!(find("Films/Inception").unwrap().locked.is_none());
        assert_eq!(find("Films/Amelie").unwrap().layout, Layout::DvdFolder);
        // aucun fichier interne d'une structure disque n'est listé
        assert!(items.iter().all(|i| !i.path.to_string_lossy().contains("STREAM")));

        // ouvrir directement un dossier disque : verrouillé
        let only = scan(&t.join("Films/Inception"));
        assert_eq!(only.len(), 1);
        assert!(only[0].locked.is_some());
        let _ = fs::remove_dir_all(&t);
    }
}
