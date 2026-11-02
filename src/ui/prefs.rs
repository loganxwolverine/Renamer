use super::window::{after_prefs, Ui};
use crate::core::model::*;
use crate::core::naming;
use adw::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;

/// Média d'exemple pour l'aperçu en direct des modèles.
fn sample(kind: MediaKind) -> MediaItem {
    let mut it = crate::core::scan::make_item(
        PathBuf::from(match kind {
            MediaKind::Movie => "/Media/Films/x.mkv",
            MediaKind::Episode => "/Media/Series/x/S01E01.mkv",
        }),
        Layout::File,
        &PathBuf::from("/Media"),
    );
    it.kind = kind;
    it.tech = TechInfo {
        source: Some("UHD Bluray".into()),
        resolution: Some("2160p".into()),
        codec: Some("HEVC".into()),
        bit_depth: Some(10),
        hdr: vec!["DV P7".into(), "HDR10".into()],
        audio: vec![AudioTrack { format: "TrueHD Atmos".into(), channels: "7.1".into(), lang: None, score: 1 }],
        langs: Some("MULTi".into()),
        ..Default::default()
    };
    match kind {
        MediaKind::Movie => {
            it.matched = Some(TmdbMatch {
                id: 238,
                kind,
                title: "Le Parrain".into(),
                original_title: "The Godfather".into(),
                original_language: "en".into(),
                year: Some(1972),
                overview: String::new(),
                poster_path: None,
            });
        }
        MediaKind::Episode => {
            it.guess.season = Some(1);
            it.guess.episodes = vec![1];
            it.episode_title = Some("Chute libre".into());
            it.matched = Some(TmdbMatch {
                id: 1396,
                kind,
                title: "Breaking Bad".into(),
                original_title: "Breaking Bad".into(),
                original_language: "en".into(),
                year: Some(2008),
                overview: String::new(),
                poster_path: None,
            });
        }
    }
    it
}

pub fn show(ui: &Rc<Ui>) {
    let cfg = ui.state.borrow().cfg.clone();
    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Préférences");

    // ---- Général ----
    let page = adw::PreferencesPage::builder().title("Général").icon_name("preferences-system-symbolic").build();
    let g = adw::PreferencesGroup::builder()
        .title("TMDB")
        .description("Clé API gratuite : themoviedb.org → Paramètres → API. La clé v3 et le jeton de lecture v4 sont acceptés.")
        .build();
    let key = adw::PasswordEntryRow::builder().title("Clé API TMDB").text(cfg.tmdb_key.as_str()).build();
    let lang = adw::EntryRow::builder().title("Langue des titres (ex. fr-FR, fr-CA)").text(cfg.language.as_str()).build();
    g.add(&key);
    g.add(&lang);
    page.add(&g);

    let g2 = adw::PreferencesGroup::builder().title("Organisation").build();
    let per_movie = adw::SwitchRow::builder()
        .title("Réorganiser l'arborescence")
        .subtitle("Désactivé : chaque fichier est renommé à sa place, aucun dossier n'est créé ni déplacé.\nActivé : rangement en « Film (année)/ » et « Série/Saison XX/ » selon les modèles de dossiers.")
        .active(cfg.reorganize)
        .build();
    let latin = adw::SwitchRow::builder().title("Titre original en alphabet latin uniquement").subtitle("Évite par exemple « 千と千尋の神隠し » dans les noms").active(cfg.original_title_latin_only).build();
    let threads = adw::SpinRow::with_range(1.0, 16.0, 1.0);
    threads.set_title("Analyses en parallèle");
    threads.set_subtitle("À réduire (1-2) si le partage réseau est lent");
    threads.set_value(cfg.probe_threads as f64);
    g2.add(&per_movie);
    g2.add(&latin);
    g2.add(&threads);
    page.add(&g2);
    dialog.add(&page);

    // ---- Modèles ----
    let page2 = adw::PreferencesPage::builder().title("Modèles de noms").icon_name("document-edit-symbolic").build();
    let help = adw::PreferencesGroup::builder()
        .title("Variables")
        .description(
            "{titre} {titre_original} {annee} {tmdbid} · {serie} {saison} {episode} {titre_episode}\n\
             {source} {resolution} {codec} {hdr} {audio} {langues} {bits} · {tech} = composition technique\n\
             Une partie entre &lt; &gt; disparaît si l'une de ses variables est vide : &lt; [{titre_original}]&gt;",
        )
        .build();
    page2.add(&help);

    let gm = adw::PreferencesGroup::builder().title("Films").build();
    let movie_folder = adw::EntryRow::builder().title("Dossier du film").text(cfg.movie_folder.as_str()).build();
    let movie_file = adw::EntryRow::builder().title("Fichier / dossier disque").text(cfg.movie_file.as_str()).build();
    let movie_prev = gtk::Label::builder().xalign(0.0).wrap(true).wrap_mode(gtk::pango::WrapMode::Char).selectable(true).margin_top(8).build();
    movie_prev.add_css_class("newname");
    gm.add(&movie_folder);
    gm.add(&movie_file);
    gm.add(&movie_prev);
    page2.add(&gm);

    let gs = adw::PreferencesGroup::builder().title("Séries").build();
    let series_folder = adw::EntryRow::builder().title("Dossier de la série").text(cfg.series_folder.as_str()).build();
    let season_folder = adw::EntryRow::builder().title("Dossier de saison").text(cfg.season_folder.as_str()).build();
    let episode_file = adw::EntryRow::builder().title("Fichier d'épisode").text(cfg.episode_file.as_str()).build();
    let ep_prev = gtk::Label::builder().xalign(0.0).wrap(true).wrap_mode(gtk::pango::WrapMode::Char).selectable(true).margin_top(8).build();
    ep_prev.add_css_class("newname");
    gs.add(&series_folder);
    gs.add(&season_folder);
    gs.add(&episode_file);
    gs.add(&ep_prev);
    page2.add(&gs);

    let gt = adw::PreferencesGroup::builder().title("Composition technique {tech}").build();
    let tech = adw::EntryRow::builder().title("Ordre des informations").text(cfg.tech.as_str()).build();
    let reset = gtk::Button::builder().label("Valeurs par défaut").halign(gtk::Align::Start).margin_top(12).build();
    gt.add(&tech);
    gt.add(&reset);
    page2.add(&gt);
    dialog.add(&page2);

    // lecture de l'état des champs
    let collect = {
        let (key, lang, per_movie, latin, threads) = (key.clone(), lang.clone(), per_movie.clone(), latin.clone(), threads.clone());
        let (mf, mfi, sf, sef, ef, te) = (movie_folder.clone(), movie_file.clone(), series_folder.clone(), season_folder.clone(), episode_file.clone(), tech.clone());
        let base = cfg.clone();
        move || {
            let mut c = base.clone();
            c.tmdb_key = key.text().trim().to_string();
            c.language = lang.text().trim().to_string();
            c.reorganize = per_movie.is_active();
            c.original_title_latin_only = latin.is_active();
            c.probe_threads = threads.value() as usize;
            c.movie_folder = mf.text().to_string();
            c.movie_file = mfi.text().to_string();
            c.series_folder = sf.text().to_string();
            c.season_folder = sef.text().to_string();
            c.episode_file = ef.text().to_string();
            c.tech = te.text().to_string();
            c
        }
    };
    let collect = Rc::new(collect);

    let update_preview = {
        let collect = collect.clone();
        let (mp, ep) = (movie_prev.clone(), ep_prev.clone());
        move || {
            let c = collect();
            let m = naming::target_path(&sample(MediaKind::Movie), &c);
            let e = naming::target_path(&sample(MediaKind::Episode), &c);
            let strip = |p: PathBuf| p.strip_prefix("/Media").map(|x| x.display().to_string()).unwrap_or_default();
            mp.set_label(&format!("Exemple : {}", strip(m)));
            ep.set_label(&format!("Exemple : {}", strip(e)));
        }
    };
    let update_preview = Rc::new(update_preview);
    update_preview();
    for e in [&movie_folder, &movie_file, &series_folder, &season_folder, &episode_file, &tech] {
        let up = update_preview.clone();
        e.connect_changed(move |_| up());
    }
    for s in [&per_movie, &latin] {
        let up = update_preview.clone();
        s.connect_active_notify(move |_| up());
    }
    {
        let d = crate::core::config::Config::default();
        let (mf, mfi, sf, sef, ef, te) = (movie_folder.clone(), movie_file.clone(), series_folder.clone(), season_folder.clone(), episode_file.clone(), tech.clone());
        reset.connect_clicked(move |_| {
            mf.set_text(&d.movie_folder);
            mfi.set_text(&d.movie_file);
            sf.set_text(&d.series_folder);
            sef.set_text(&d.season_folder);
            ef.set_text(&d.episode_file);
            te.set_text(&d.tech);
        });
    }

    let u = ui.clone();
    dialog.connect_closed(move |_| {
        u.state.borrow_mut().cfg = collect();
        after_prefs(&u);
    });
    dialog.present(Some(&ui.window));
}
