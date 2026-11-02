//! Exécution du plan, journal et annulation.

use super::config::Config;
use super::naming::{ItemPlan, Op};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Journal {
    pub date: String,
    pub root: PathBuf,
    /// Opérations effectivement réalisées, dans l'ordre
    pub done: Vec<Op>,
}

pub fn journal_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(Config::dir).join("openhtpc-renamer").join("journal")
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub ok: Vec<(usize, PathBuf)>,
    pub failed: Vec<(usize, String)>,
    pub journal: Option<PathBuf>,
}

fn write_journal(path: &Path, j: &Journal) {
    if let Some(d) = path.parent() {
        let _ = fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_string_pretty(j) {
        let _ = fs::write(path, s);
    }
}

fn exec(op: &Op) -> Result<bool, String> {
    match op {
        Op::Mkdir(p) => {
            if p.exists() {
                return Ok(false);
            }
            fs::create_dir(p).map_err(|e| format!("Création de {} : {e}", p.display()))?;
            Ok(true)
        }
        Op::Rename { from, to } => {
            if from == to {
                return Ok(false);
            }
            // Ne jamais écraser (sauf simple changement de casse)
            let case_only = from.to_string_lossy().to_lowercase() == to.to_string_lossy().to_lowercase();
            if to.exists() && !case_only {
                return Err(format!("La destination existe déjà : {}", to.display()));
            }
            if !from.exists() {
                return Err(format!("Introuvable : {}", from.display()));
            }
            if case_only {
                let tmp = from.with_file_name(format!(".ohtpc-tmp-{}", std::process::id()));
                fs::rename(from, &tmp).and_then(|_| fs::rename(&tmp, to))
            } else {
                fs::rename(from, to)
            }
            .map_err(|e| format!("{} → {} : {e}", from.display(), to.display()))?;
            Ok(true)
        }
        Op::Rmdir(p) => {
            fs::remove_dir(p).map_err(|e| e.to_string())?;
            Ok(true)
        }
    }
}

/// Un dossier ne contenant plus que des fichiers parasites (Thumbs.db, .DS_Store…) est considéré vide.
fn is_effectively_empty(d: &Path) -> bool {
    match fs::read_dir(d) {
        Ok(rd) => rd.flatten().all(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            n == "thumbs.db" || n == ".ds_store" || n == "desktop.ini"
        }),
        Err(_) => false,
    }
}

pub fn apply(plans: &[ItemPlan], root: &Path) -> Outcome {
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let jpath = journal_dir().join(format!("{stamp}.json"));
    let mut j = Journal { date: stamp, root: root.to_path_buf(), done: vec![] };
    let mut out = Outcome::default();

    for plan in plans {
        let mut err = None;
        for op in &plan.ops {
            match exec(op) {
                Ok(true) => j.done.push(op.clone()),
                Ok(false) => {}
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        if err.is_none() {
            // nettoyage des anciens dossiers devenus vides
            for start in &plan.cleanup {
                let mut d = start.clone();
                while d.starts_with(&plan.stop_dir) && d != plan.stop_dir && d.is_dir() && is_effectively_empty(&d) {
                    if let Ok(rd) = fs::read_dir(&d) {
                        for e in rd.flatten() {
                            let _ = fs::remove_file(e.path());
                        }
                    }
                    if exec(&Op::Rmdir(d.clone())).is_ok() {
                        j.done.push(Op::Rmdir(d.clone()));
                    } else {
                        break;
                    }
                    match d.parent() {
                        Some(p) => d = p.to_path_buf(),
                        None => break,
                    }
                }
            }
        }
        match err {
            None => out.ok.push((plan.index, plan.target.clone())),
            Some(e) => out.failed.push((plan.index, e)),
        }
        write_journal(&jpath, &j);
    }
    if !j.done.is_empty() {
        out.journal = Some(jpath);
    } else {
        let _ = fs::remove_file(&jpath);
    }
    out
}

pub fn last_journal() -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(journal_dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
        .collect();
    v.sort();
    v.pop()
}

/// Annule un journal (ordre inverse). Renvoie (opérations annulées, erreurs).
pub fn undo(path: &Path) -> Result<(usize, Vec<String>), String> {
    let s = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let j: Journal = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    let mut n = 0;
    let mut errs = vec![];
    for op in j.done.iter().rev() {
        let inv = match op {
            Op::Mkdir(p) => Op::Rmdir(p.clone()),
            Op::Rmdir(p) => Op::Mkdir(p.clone()),
            Op::Rename { from, to } => Op::Rename { from: to.clone(), to: from.clone() },
        };
        match exec(&inv) {
            Ok(_) => n += 1,
            Err(e) => errs.push(e),
        }
    }
    let done = path.with_extension("annule");
    let _ = fs::rename(path, done);
    Ok((n, errs))
}
