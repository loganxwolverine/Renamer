mod core;
mod ui;

use crate::core::{config::Config, engine, naming, scan, tmdb::Tmdb};
use std::path::PathBuf;

const APP_ID: &str = "io.github.openhtpc.Renamer";

fn main() -> gtk::glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    // Mode texte : aperçu sans rien renommer
    //   openhtpc-renamer --apercu /chemin/Media [--sans-tmdb]
    if args.get(1).map(|a| a == "--apercu" || a == "--dry-run").unwrap_or(false) {
        let root = PathBuf::from(args.get(2).expect("dossier manquant"));
        cli_preview(root, args.iter().any(|a| a == "--sans-tmdb"));
        return gtk::glib::ExitCode::SUCCESS;
    }
    ui::run(APP_ID)
}

fn cli_preview(root: PathBuf, offline: bool) {
    let mut cfg = Config::load();
    if let Ok(k) = std::env::var("TMDB_API_KEY") {
        cfg.tmdb_key = k;
    }
    let mut items = scan::scan(&root);
    eprintln!("{} médias trouvés", items.len());
    let quiet = |_p: engine::Progress| {};
    engine::probe_all(&mut items, cfg.probe_threads, &quiet);
    let api = Tmdb::new(&cfg.tmdb_key, &cfg.language);
    if !offline && api.has_key() {
        engine::identify_all(&mut items, &api, &cfg, &quiet);
    }
    let idx: Vec<usize> = (0..items.len()).collect();
    for plan in naming::build_plan(&items, &idx, &cfg) {
        let it = &items[plan.index];
        println!("[{}] {:?} {}", it.status.label(), it.kind, it.layout.label());
        println!("   avant : {}", it.path.strip_prefix(&root).unwrap_or(&it.path).display());
        println!("   après : {}", plan.target.strip_prefix(&root).unwrap_or(&plan.target).display());
        if let Some(e) = &it.tech.probe_error {
            println!("   (ffprobe : {e})");
        }
    }
}
