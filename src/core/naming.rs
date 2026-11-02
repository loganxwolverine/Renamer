//! Modèles de nommage et construction du plan de renommage.

use super::config::Config;
use super::model::*;
use super::parse::normalize;
use super::scan;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

static PLACEHOLDER: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{([a-z_]+)\}").unwrap());
static OPTIONAL: Lazy<Regex> = Lazy::new(|| Regex::new(r"<([^<>]*)>").unwrap());
static SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
static EMPTY_BRACKETS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\(\s*\)|\[\s*\]|\s+-\s*$|^\s*-\s+").unwrap());

/// Remplace `{clé}` ; une section `<…>` disparaît si l'une de ses variables est vide.
pub fn render(tpl: &str, vars: &HashMap<&str, String>) -> String {
    let get = |k: &str| vars.get(k).cloned().unwrap_or_default();
    let with_opt = OPTIONAL.replace_all(tpl, |c: &regex::Captures| {
        let inner = &c[1];
        let any_empty = PLACEHOLDER.captures_iter(inner).any(|p| get(&p[1]).trim().is_empty());
        if any_empty { String::new() } else { inner.to_string() }
    });
    let out = PLACEHOLDER.replace_all(&with_opt, |c: &regex::Captures| get(&c[1]));
    let out = EMPTY_BRACKETS.replace_all(&out, "");
    SPACES.replace_all(out.trim(), " ").to_string()
}

/// Nom sûr pour Linux, SMB/Windows et la plupart des lecteurs.
pub fn sanitize(name: &str) -> String {
    let s = name
        .replace(": ", " - ")
        .replace(':', "-")
        .replace(['/', '\\', '|'], "-")
        .replace(['?', '*', '"', '<', '>'], "")
        ;
    let s = s.chars().filter(|c| !c.is_control()).collect::<String>();
    let s = SPACES.replace_all(&s, " ");
    let s = s.trim().trim_end_matches(['.', ' ']).to_string();
    // limite raisonnable (255 octets max par composant)
    let mut out = String::new();
    for c in s.chars() {
        if out.len() + c.len_utf8() > 200 {
            break;
        }
        out.push(c);
    }
    out.trim().to_string()
}

fn is_latin(s: &str) -> bool {
    s.chars().filter(|c| c.is_alphabetic()).all(|c| (c as u32) < 0x0250 || ('\u{1E00}'..='\u{1EFF}').contains(&c))
}

pub fn vars_for(item: &MediaItem, cfg: &Config) -> HashMap<&'static str, String> {
    let mut v = HashMap::new();
    let t = &item.tech;
    v.insert("source", t.source.clone().unwrap_or_default());
    v.insert("resolution", t.resolution.clone().unwrap_or_default());
    v.insert("codec", t.codec.clone().unwrap_or_default());
    v.insert("hdr", t.hdr_label());
    v.insert("audio", t.audio_label());
    v.insert("langues", t.langs.clone().unwrap_or_default());
    v.insert("bits", t.bit_depth.filter(|b| *b > 8).map(|b| format!("{b}bit")).unwrap_or_default());
    let tech = render(&cfg.tech, &v);
    v.insert("tech", tech);

    let (title, orig, year, id) = match &item.matched {
        Some(m) => {
            let orig = if normalize(&m.original_title) == normalize(&m.title)
                || (cfg.original_title_latin_only && !is_latin(&m.original_title))
            {
                String::new()
            } else {
                m.original_title.clone()
            };
            (m.title.clone(), orig, m.year.or(item.guess.year), m.id.to_string())
        }
        None => (item.guess.title.clone(), String::new(), item.guess.year, String::new()),
    };
    v.insert("titre", title.clone());
    v.insert("serie", title);
    v.insert("titre_original", orig);
    v.insert("annee", year.map(|y| y.to_string()).unwrap_or_default());
    v.insert("tmdbid", id);
    v.insert("saison", item.guess.season.map(|s| format!("{s:02}")).unwrap_or_default());
    v.insert(
        "episode",
        item.guess.episodes.iter().map(|e| format!("{e:02}")).collect::<Vec<_>>().join("-E"),
    );
    v.insert("titre_episode", item.episode_title.clone().unwrap_or_default());
    v
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Op {
    Mkdir(PathBuf),
    Rename { from: PathBuf, to: PathBuf },
    Rmdir(PathBuf),
}

#[derive(Debug, Clone)]
pub struct ItemPlan {
    pub index: usize,
    pub ops: Vec<Op>,
    /// Emplacement final du média
    pub target: PathBuf,
    /// Dossiers qui pourront être supprimés s'ils sont vides après déplacement
    pub cleanup: Vec<PathBuf>,
    pub stop_dir: PathBuf,
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default()
}

fn rel_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Nom cible du média seul (fichier avec extension, ou dossier disque).
pub fn target_name(item: &MediaItem, cfg: &Config, vars: &HashMap<&str, String>) -> String {
    let tpl = match item.kind {
        MediaKind::Movie => &cfg.movie_file,
        MediaKind::Episode => &cfg.episode_file,
    };
    let base = sanitize(&render(tpl, vars));
    if item.layout.is_dir() { base } else { format!("{base}{}", ext_of(&item.path)) }
}

/// Chemin final d'un média (sans tenir compte des conflits).
pub fn target_path(item: &MediaItem, cfg: &Config) -> PathBuf {
    let vars = vars_for(item, cfg);
    let name = target_name(item, cfg, &vars);
    match item.kind {
        MediaKind::Movie if cfg.folder_per_movie => {
            item.category_dir.join(sanitize(&render(&cfg.movie_folder, &vars))).join(name)
        }
        MediaKind::Movie => item.path.parent().unwrap_or(&item.category_dir).join(name),
        MediaKind::Episode => item
            .category_dir
            .join(sanitize(&render(&cfg.series_folder, &vars)))
            .join(sanitize(&render(&cfg.season_folder, &vars)))
            .join(name),
    }
}

fn unique(p: PathBuf, claimed: &HashSet<PathBuf>, current: &Path) -> PathBuf {
    let taken = |q: &Path| claimed.contains(q) || (q.exists() && q != current);
    if !taken(&p) {
        return p;
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = if p.is_dir() || current.is_dir() { String::new() } else { ext_of(&p) };
    let stem = if ext.is_empty() { rel_name(&p) } else { stem };
    for n in 2..100 {
        let q = p.with_file_name(format!("{stem} ({n}){ext}"));
        if !taken(&q) {
            return q;
        }
    }
    p
}

/// Ajoute le renommage du média et de ses fichiers annexes (sous-titres, nfo…).
fn push_media_moves(ops: &mut Vec<Op>, from: &Path, to: &Path, is_dir: bool) {
    if from == to {
        return;
    }
    ops.push(Op::Rename { from: from.to_path_buf(), to: to.to_path_buf() });
    if is_dir {
        return;
    }
    let old_stem = from.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let new_stem = to.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let new_dir = to.parent().unwrap();
    for sc in scan::sidecars(from) {
        let n = rel_name(&sc);
        let rest = &n[old_stem.len()..];
        ops.push(Op::Rename { from: sc.clone(), to: new_dir.join(format!("{new_stem}{rest}")) });
    }
}

pub fn build_plan(items: &[MediaItem], indices: &[usize], cfg: &Config) -> Vec<ItemPlan> {
    let mut claimed: HashSet<PathBuf> = HashSet::new();
    let mut moved_dirs: HashSet<PathBuf> = HashSet::new();
    let mut created: HashSet<PathBuf> = HashSet::new();
    let mut plans = vec![];

    for &i in indices {
        let item = &items[i];
        let is_dir = item.layout.is_dir();
        let wanted = target_path(item, cfg);
        let mut ops = vec![];
        let mut cleanup = vec![];
        let parent = item.path.parent().unwrap().to_path_buf();

        // Film seul dans son propre dossier : on renomme le dossier plutôt que d'en créer un.
        let target_dir = wanted.parent().unwrap().to_path_buf();
        let own_folder = item.kind == MediaKind::Movie
            && cfg.folder_per_movie
            && parent != item.category_dir
            && parent.starts_with(&item.category_dir)
            && !moved_dirs.contains(&parent)
            && scan::count_videos(&parent) == 1;

        let target;
        if own_folder && (target_dir == parent || (!target_dir.exists() && !claimed.contains(&target_dir) && !created.contains(&target_dir))) {
            if target_dir != parent {
                ops.push(Op::Rename { from: parent.clone(), to: target_dir.clone() });
                moved_dirs.insert(parent.clone());
                if let Some(gp) = parent.parent() {
                    cleanup.push(gp.to_path_buf());
                }
            }
            let cur_inside = target_dir.join(rel_name(&item.path));
            target = unique(wanted, &claimed, &cur_inside);
            // les annexes se trouvent encore à l'ancien emplacement au moment de la planification
            let mut tmp = vec![];
            push_media_moves(&mut tmp, &item.path, &target, is_dir);
            for op in tmp {
                if let Op::Rename { from, to } = op {
                    ops.push(Op::Rename { from: target_dir.join(rel_name(&from)), to });
                }
            }
        } else {
            target = unique(wanted, &claimed, &item.path);
            let tdir = target.parent().unwrap().to_path_buf();
            // dossiers à créer (série / saison / film)
            let mut missing = vec![];
            let mut d = tdir.clone();
            while !d.exists() && !created.contains(&d) {
                missing.push(d.clone());
                match d.parent() {
                    Some(p) => d = p.to_path_buf(),
                    None => break,
                }
            }
            for m in missing.into_iter().rev() {
                created.insert(m.clone());
                ops.push(Op::Mkdir(m));
            }
            push_media_moves(&mut ops, &item.path, &target, is_dir);
            if tdir != parent {
                cleanup.push(parent.clone());
            }
        }
        claimed.insert(target.clone());
        claimed.insert(target.parent().unwrap().to_path_buf());
        plans.push(ItemPlan { index: i, ops, target, cleanup, stop_dir: item.category_dir.clone() });
    }
    plans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, String> {
        pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    #[test]
    fn optional_sections() {
        let tpl = "{titre} ({annee})< [{titre_original}]> [tmdbid-{tmdbid}]";
        let v = vars(&[("titre", "Le Parrain"), ("annee", "1972"), ("titre_original", "The Godfather"), ("tmdbid", "238")]);
        assert_eq!(render(tpl, &v), "Le Parrain (1972) [The Godfather] [tmdbid-238]");
        let v = vars(&[("titre", "Amélie"), ("annee", "2001"), ("titre_original", ""), ("tmdbid", "194")]);
        assert_eq!(render(tpl, &v), "Amélie (2001) [tmdbid-194]");
    }

    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize("Mission: Impossible"), "Mission - Impossible");
        assert_eq!(sanitize("Qui veut la peau de Roger Rabbit?"), "Qui veut la peau de Roger Rabbit");
        assert_eq!(sanitize("AC/DC Live..."), "AC-DC Live");
    }
}
