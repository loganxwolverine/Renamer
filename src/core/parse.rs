//! Analyse « à la guessit » des noms de fichiers et de dossiers.

use super::model::Guess;
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

/// Mots qui marquent la fin du titre dans un nom de release.
const STOP_TAGS: &[&str] = &[
    "480p", "576p", "720p", "1080p", "1080i", "2160p", "4k", "uhd", "bluray", "blu-ray", "bdrip",
    "brrip", "bdremux", "remux", "bd", "bd25", "bd50", "bd66", "bd100", "web", "web-dl", "webdl",
    "webrip", "hdtv", "dvdrip", "dvd", "dvd5", "dvd9", "dvdr", "hdlight", "mhd", "x264", "x265",
    "h264", "h265", "h.264", "h.265", "hevc", "avc", "av1", "xvid", "divx", "vc1", "vc-1", "mpeg2",
    "10bit", "8bit", "hdr", "hdr10", "hdr10+", "hdr10plus", "dv", "dovi", "hlg", "sdr", "dts",
    "dts-hd", "dtshd", "dts-x", "dtsx", "truehd", "atmos", "ac3", "eac3", "dd", "dd5", "ddp",
    "ddp5", "dd+", "aac", "flac", "lpcm", "pcm", "opus", "french", "truefrench", "vff", "vfq",
    "vfi", "vf", "vf2", "vo", "vost", "vostfr", "subfrench", "multi", "multi-vf2", "proper",
    "repack", "extended", "unrated", "uncut", "directors", "director's", "imax", "internal",
    "complete", "integrale", "intégrale", "limited", "remastered", "restored", "criterion",
    "custom", "hybrid", "nf", "amzn", "dsnp", "atvp", "hmax", "cr", "mkv", "mp4", "avi", "iso",
];

/// Noms de fichiers « vides de sens » (MakeMKV, rips de disques…) : on prend alors le dossier.
static GENERIC_NAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(title[ _-]?t?\d*|t\d{1,3}|track ?\d*|vts[ _]?\d*[ _]?\d*|disc ?\d*|disk ?\d*|cd ?\d*|dvd ?\d*|bd ?\d*|bdmv|video_ts|movie|film|video|main|feature|sample|\d{1,5}|[a-z0-9]{1,3})$").unwrap()
});

static SEASON_DIR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(?:season|saison|series|s|staffel|temporada|livre|book)[ ._-]*(\d{1,3})\b|^(?:sp[eé]cials?|extras?)$").unwrap()
});

static EP_SXXEXX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?:^|[^a-z0-9])s(\d{1,3})[ ._-]?e(\d{1,4})((?:[ ._-]?-?[ ._-]?e\d{1,4})*)").unwrap()
});
static EP_NXNN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(?:^|[^a-z0-9])(\d{1,2})x(\d{2,3})(?:[^a-z0-9]|$)").unwrap());
static EP_ONLY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?:^|[^a-z0-9])(?:e|ep|episode|épisode)[ ._-]?(\d{1,4})(?:[^0-9]|$)").unwrap()
});
static EP_LEADING_NUM: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(\d{1,3})(?:[ ._-]+|$)").unwrap());
static EXTRA_EP: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)e(\d{1,4})").unwrap());
static YEAR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b((?:19|20)\d{2})\b").unwrap());
static BRACKETS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[[^\]]*\]|\{[^}]*\}").unwrap());
static TMDB_ID: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)[\[{(]\s*tmdb(?:id)?[-= :]?(\d+)\s*[\]})]").unwrap());
static SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

/// Minuscule, sans accents, sans ponctuation : pour comparer des titres.
pub fn normalize(s: &str) -> String {
    let s: String = s
        .nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
        .replace('&', " and ")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    SPACES.replace_all(s.trim(), " ").into_owned()
}

/// Identifiant TMDB déjà présent dans un nom (`[tmdbid-123]`, `{tmdb-123}`).
pub fn find_tmdb_id(s: &str) -> Option<u64> {
    TMDB_ID.captures(s).and_then(|c| c[1].parse().ok())
}

fn is_stop_tag(tok: &str) -> bool {
    let t = tok.to_lowercase();
    let t = t.trim_matches(|c: char| !c.is_alphanumeric() && c != '+');
    STOP_TAGS.contains(&t)
}

/// Remplace les séparateurs de release (points, underscores) par des espaces,
/// en conservant les abréviations du type « S.W.A.T. ».
fn despace(s: &str) -> String {
    let s = s.replace('_', " ");
    let dots = s.matches('.').count();
    let spaces = s.matches(' ').count();
    let s = if dots > spaces {
        // garder « 5.1 » / « 7.1 » ; le reste devient espace
        let re = Regex::new(r"(\d)\.(\d)").unwrap();
        let protected = re.replace_all(&s, "$1\u{1}$2");
        protected.replace('.', " ").replace('\u{1}', ".")
    } else {
        s
    };
    SPACES.replace_all(s.trim(), " ").into_owned()
}

fn clean_title(s: &str) -> String {
    let s = s.trim_matches(|c: char| c.is_whitespace() || "-_.([{,;".contains(c));
    let s = SPACES.replace_all(s, " ");
    s.trim().to_string()
}

/// Analyse d'un nom (sans extension).
pub fn parse_name(raw: &str) -> Guess {
    let mut g = Guess::default();
    let no_id = TMDB_ID.replace_all(raw, " ");
    let text = despace(&no_id);

    // Tags techniques (pour secours)
    for tok in text.split(' ') {
        if is_stop_tag(tok) {
            g.tags.push(tok.to_lowercase());
        }
    }

    // Episodes
    let mut cut = text.len();
    if let Some(c) = EP_SXXEXX.captures(&text) {
        g.season = c[1].parse().ok();
        g.episodes.push(c[2].parse().unwrap_or(0));
        for e in EXTRA_EP.captures_iter(c.get(3).map(|m| m.as_str()).unwrap_or("")) {
            if let Ok(n) = e[1].parse() {
                g.episodes.push(n);
            }
        }
        cut = cut.min(c.get(0).unwrap().start());
    } else if let Some(c) = EP_NXNN.captures(&text) {
        g.season = c[1].parse().ok();
        g.episodes.push(c[2].parse().unwrap_or(0));
        cut = cut.min(c.get(0).unwrap().start());
    } else if let Some(c) = EP_ONLY.captures(&text) {
        g.episodes.push(c[1].parse().unwrap_or(0));
        cut = cut.min(c.get(0).unwrap().start());
    }

    // Année : on prend la dernière qui n'est pas en tout début (ex. « 2001 A Space Odyssey 1968 »)
    let mut year_pos = None;
    for c in YEAR.captures_iter(&text) {
        let m = c.get(1).unwrap();
        if m.start() == 0 && text.len() > 4 {
            continue;
        }
        if m.start() > cut + 8 && g.is_episode() {
            continue;
        }
        let y: u16 = m.as_str().parse().unwrap();
        if (1900..=2035).contains(&y) {
            g.year = Some(y);
            year_pos = Some(m.start());
        }
    }
    if let Some(p) = year_pos {
        cut = cut.min(p);
    }

    // Premier tag technique
    let mut pos = 0;
    for tok in text.split(' ') {
        if pos > 0 && is_stop_tag(tok) {
            cut = cut.min(pos);
            break;
        }
        pos += tok.len() + 1;
    }

    let head = &text[..cut.min(text.len())];
    let head = BRACKETS.replace_all(head, " ");
    g.title = clean_title(&head);
    g
}

fn stem_of(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn is_generic(title: &str) -> bool {
    let t = title.trim();
    t.is_empty() || GENERIC_NAME.is_match(t)
}

pub fn season_from_dir(name: &str) -> Option<u16> {
    let c = SEASON_DIR.captures(name.trim())?;
    match c.get(1) {
        Some(m) => m.as_str().parse().ok(),
        None => Some(0), // Specials
    }
}

/// Analyse complète d'un média : nom + contexte des dossiers parents.
/// `media` = fichier vidéo / iso, ou dossier disque ; `stop` = dossier catégorie.
pub fn guess_for(media: &Path, is_dir: bool, stop: &Path) -> Guess {
    let base = if is_dir { name_of(media) } else { stem_of(media) };
    let mut g = parse_name(&base);

    // Ancêtres (du plus proche au plus lointain), sans dépasser le dossier catégorie
    let ancestors: Vec<&Path> = media
        .ancestors()
        .skip(1)
        .take_while(|a| *a != stop && a.starts_with(stop))
        .collect();

    // Saison depuis un dossier « Saison 2 »
    if g.is_episode() && g.season.is_none() {
        for a in &ancestors {
            if let Some(s) = season_from_dir(&name_of(a)) {
                g.season = Some(s);
                break;
            }
        }
    }

    // Fichier nommé « 01 - Pilote.mkv » dans un dossier « Saison 1 »
    if !g.is_episode() {
        if let Some(parent) = ancestors.first() {
            if let Some(s) = season_from_dir(&name_of(parent)) {
                if let Some(c) = EP_LEADING_NUM.captures(&base) {
                    g.season = Some(s);
                    g.episodes.push(c[1].parse().unwrap_or(0));
                    g.title.clear();
                }
            }
        }
    }

    let title_is_weak = is_generic(&g.title) || (g.is_episode() && g.title.len() < 2);
    if title_is_weak || g.year.is_none() {
        // Remonter les dossiers pour trouver un meilleur titre / l'année
        for a in &ancestors {
            let n = name_of(a);
            if season_from_dir(&n).is_some() {
                continue;
            }
            let pg = parse_name(&n);
            if is_generic(&pg.title) {
                continue;
            }
            if title_is_weak {
                g.title = pg.title.clone();
                if g.year.is_none() {
                    g.year = pg.year;
                }
                for t in pg.tags {
                    if !g.tags.contains(&t) {
                        g.tags.push(t);
                    }
                }
            } else if g.year.is_none()
                && pg.year.is_some()
                && normalize(&pg.title) == normalize(&g.title)
            {
                g.year = pg.year;
            }
            break;
        }
    }
    g
}

/// Cherche un identifiant TMDB dans le nom du média ou ses dossiers parents.
pub fn tmdb_id_for(media: &Path, stop: &Path) -> Option<u64> {
    media
        .ancestors()
        .take_while(|a| *a != stop)
        .find_map(|a| find_tmdb_id(&name_of(a)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_movie() {
        let g = parse_name("Le.Diner.de.Cons.1998.MULTi.1080p.BluRay.x264-GRP");
        assert_eq!(g.title, "Le Diner de Cons");
        assert_eq!(g.year, Some(1998));
        assert!(!g.is_episode());
    }

    #[test]
    fn title_with_number_year() {
        let g = parse_name("2001 A Space Odyssey (1968) 2160p UHD");
        assert_eq!(g.title, "2001 A Space Odyssey");
        assert_eq!(g.year, Some(1968));
        let g = parse_name("Blade Runner 2049 2017 MULTi 2160p");
        assert_eq!(g.title, "Blade Runner 2049");
        assert_eq!(g.year, Some(2017));
    }

    #[test]
    fn episodes() {
        let g = parse_name("Breaking.Bad.S01E01.Pilot.1080p.BluRay");
        assert_eq!(g.title, "Breaking Bad");
        assert_eq!(g.season, Some(1));
        assert_eq!(g.episodes, vec![1]);
        let g = parse_name("The Office (US) - s02e03-e04 - Office Olympics");
        assert_eq!(g.title, "The Office (US)");
        assert_eq!(g.episodes, vec![3, 4]);
        let g = parse_name("Kaamelott 3x05");
        assert_eq!(g.title, "Kaamelott");
        assert_eq!((g.season, g.episodes.clone()), (Some(3), vec![5]));
    }

    #[test]
    fn spaces_and_brackets() {
        let g = parse_name("[GRP] Amélie (2001) [1080p]");
        assert_eq!(g.title, "Amélie");
        assert_eq!(g.year, Some(2001));
        let g = parse_name("Alien - Le huitième passager (1979) [tmdbid-348]");
        assert_eq!(g.title, "Alien - Le huitième passager");
        assert_eq!(find_tmdb_id("Alien (1979) [tmdbid-348]"), Some(348));
        assert_eq!(find_tmdb_id("Alien (1979) {tmdb-348}"), Some(348));
    }

    #[test]
    fn generic_file_uses_folder() {
        let stop = Path::new("/m/Films");
        let g = guess_for(Path::new("/m/Films/Le Parrain 1972/title_t00.mkv"), false, stop);
        assert_eq!(g.title, "Le Parrain");
        assert_eq!(g.year, Some(1972));
        let g = guess_for(Path::new("/m/Films/Heat (1995)/DISC1"), true, stop);
        assert_eq!(g.title, "Heat");
        assert_eq!(g.year, Some(1995));
    }

    #[test]
    fn episode_in_season_folder() {
        let stop = Path::new("/m/Series");
        let g = guess_for(Path::new("/m/Series/Kaamelott/Livre 1/S01E02.mkv"), false, stop);
        assert_eq!(g.title, "Kaamelott");
        let g = guess_for(Path::new("/m/Series/Dark (2017)/Saison 2/03 - Les nouveaux.mkv"), false, stop);
        assert_eq!(g.title, "Dark");
        assert_eq!(g.year, Some(2017));
        assert_eq!((g.season, g.episodes.clone()), (Some(2), vec![3]));
    }

    #[test]
    fn normalize_cmp() {
        assert_eq!(normalize("Amélie"), "amelie");
        assert_eq!(normalize("Léon : The Professional"), "leon the professional");
    }
}
