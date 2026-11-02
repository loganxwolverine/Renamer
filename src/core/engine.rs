//! Chaîne d'analyse : ffprobe en parallèle puis identification TMDB par groupe.

use super::config::Config;
use super::model::*;
use super::naming;
use super::parse;
use super::probe;
use super::tmdb::{self, Tmdb};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

pub enum Progress {
    Step { done: usize, total: usize, label: String },
    Item(usize, Box<MediaItem>),
}

pub fn probe_all(items: &mut [MediaItem], threads: usize, report: &(dyn Fn(Progress) + Sync)) {
    let total = items.len();
    let done = AtomicUsize::new(0);
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads.max(1)).build().unwrap();
    pool.install(|| {
        items.par_iter_mut().for_each(|it| {
            let mut t = probe::probe(&it.path, it.layout);
            probe::fill_from_tags(&mut t, &it.guess.tags);
            if t.source.is_none() && it.layout == Layout::Iso {
                t.source = Some("ISO".into());
            }
            it.tech = t;
            let n = done.fetch_add(1, Ordering::SeqCst) + 1;
            report(Progress::Step { done: n, total, label: format!("Analyse technique : {}", it.display_name()) });
        });
    });
}

/// Applique un résultat TMDB à un élément (titre d'épisode inclus).
pub fn apply_match(it: &mut MediaItem, m: TmdbMatch, api: &Tmdb, status: Status, cfg: &Config) {
    if it.kind == MediaKind::Episode {
        if let (Some(s), Some(e)) = (it.guess.season, it.guess.episodes.first()) {
            it.episode_title = api.episode_title(m.id, s, *e);
        }
    }
    it.matched = Some(m);
    it.status = status;
    refresh_status(it, cfg);
}

/// Un élément verrouillé reste affiché (avec son explication) mais n'est jamais prêt à renommer.
pub fn lock_status(it: &mut MediaItem) {
    if let Some(l) = &it.locked {
        it.status = Status::Locked;
        it.message = Some(l.clone());
        it.included = false;
    }
}

pub fn refresh_status(it: &mut MediaItem, cfg: &Config) {
    if it.matched.is_some() && naming::target_path(it, cfg) == it.path {
        it.status = Status::UpToDate;
    }
}

/// Identifie tous les éléments ; les épisodes d'une même série partagent la recherche.
pub fn identify_all(items: &mut [MediaItem], api: &Tmdb, cfg: &Config, report: &(dyn Fn(Progress) + Sync)) {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, it) in items.iter().enumerate() {
        groups.entry(it.group_key()).or_default().push(i);
    }
    let total = groups.len();
    for (n, (_, idxs)) in groups.into_iter().enumerate() {
        let first = &items[idxs[0]];
        report(Progress::Step { done: n, total, label: format!("TMDB : {}", first.guess.title) });
        let kind = first.kind;
        let query = first.guess.title.clone();
        let year = first.guess.year;

        let mut chosen: Option<(TmdbMatch, Status)> = None;
        let mut cands = vec![];
        let mut err = None;
        if let Some(id) = parse::tmdb_id_for(&first.path, &first.category_dir) {
            match api.by_id(kind, id) {
                Ok(m) => chosen = Some((m, Status::Confirmed)),
                Err(e) => err = Some(e),
            }
        }
        if chosen.is_none() && !query.is_empty() {
            match api.search(kind, &query, year) {
                Ok(c) => {
                    if let Some(i) = tmdb::auto_pick(&c, &query, year) {
                        chosen = Some((c[i].clone(), Status::Auto));
                    }
                    cands = c;
                }
                Err(e) => err = Some(e),
            }
        }
        for &i in &idxs {
            let it = &mut items[i];
            it.candidates = cands.clone();
            it.message = err.clone();
            match &chosen {
                Some((m, st)) => apply_match(it, m.clone(), api, *st, cfg),
                None if err.is_some() => it.status = Status::Error,
                None if cands.is_empty() => it.status = Status::NotFound,
                None => it.status = Status::Review,
            }
            lock_status(it);
            report(Progress::Item(i, Box::new(it.clone())));
        }
    }
    report(Progress::Step { done: total, total, label: "Terminé".into() });
}
