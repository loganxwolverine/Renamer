use crate::core::config::Config;
use crate::core::engine::{self, Progress};
use crate::core::model::*;
use crate::core::naming;
use crate::core::rename;
use crate::core::scan;
use crate::core::tmdb::{self, Tmdb};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Review,
    Ready,
    NotFound,
    Done,
}

const FILTERS: &[(&str, Filter)] = &[
    ("Tous", Filter::All),
    ("À vérifier", Filter::Review),
    ("Prêts", Filter::Ready),
    ("Non trouvés / erreurs", Filter::NotFound),
    ("Conformes / renommés", Filter::Done),
];

pub struct State {
    pub cfg: Config,
    pub api: Arc<Tmdb>,
    root: Option<PathBuf>,
    items: Vec<MediaItem>,
    selected: Option<usize>,
    filter: Filter,
    search: String,
    busy: bool,
    posters: HashMap<String, gdk::Texture>,
}

struct RowW {
    name: gtk::Label,
    target: gtk::Label,
    status: gtk::Label,
    icon: gtk::Image,
    check: gtk::CheckButton,
}

pub struct Ui {
    pub window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    title: adw::WindowTitle,
    stack: gtk::Stack,
    list: gtk::ListBox,
    rows: RefCell<Vec<RowW>>,
    progress: gtk::ProgressBar,
    progress_rev: gtk::Revealer,
    summary: gtk::Label,
    action_bar: gtk::ActionBar,
    rename_btn: gtk::Button,
    reanalyze_btn: gtk::Button,
    // détails
    details_stack: gtk::Stack,
    d_poster: gtk::Picture,
    d_name: gtk::Label,
    d_path: gtk::Label,
    d_chips: gtk::FlowBox,
    d_tech: adw::PreferencesGroup,
    d_tech_rows: RefCell<Vec<adw::ActionRow>>,
    d_query: adw::EntryRow,
    d_year: adw::EntryRow,
    d_kind: adw::ComboRow,
    d_id: adw::EntryRow,
    d_cands: gtk::ListBox,
    d_cands_status: gtk::Label,
    d_target: gtk::Label,
    d_include: adw::SwitchRow,
    pub state: RefCell<State>,
    /// Vrai pendant qu'on remplit les widgets par programme (ignore les signaux)
    updating: Cell<bool>,
}

struct Guard<'a>(&'a Cell<bool>, bool);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.set(self.1);
    }
}
impl Ui {
    fn guard(&self) -> Guard<'_> {
        let prev = self.updating.replace(true);
        Guard(&self.updating, prev)
    }
    fn is_updating(&self) -> bool {
        self.updating.get()
    }
}

type U = Rc<Ui>;

fn icon_for(it: &MediaItem) -> &'static str {
    match (it.kind, it.layout) {
        (MediaKind::Episode, _) => "video-display-symbolic",
        (_, Layout::DvdFolder | Layout::BlurayFolder | Layout::Iso) => "media-optical-symbolic",
        _ => "video-x-generic-symbolic",
    }
}

fn rel(p: &Path, root: &Option<PathBuf>) -> String {
    match root {
        Some(r) => p.strip_prefix(r).unwrap_or(p).display().to_string(),
        None => p.display().to_string(),
    }
}

fn chip(text: &str, extra: Option<&str>) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("chip");
    if let Some(e) = extra {
        l.add_css_class(e);
    }
    l
}

fn clear_box(lb: &gtk::ListBox) {
    while let Some(c) = lb.first_child() {
        lb.remove(&c);
    }
}

pub fn build(app: &adw::Application) {
    let cfg = Config::load();
    let api = Arc::new(Tmdb::new(&cfg.tmdb_key, &cfg.language));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("OpenHTPC Renamer")
        .default_width(1420)
        .default_height(880)
        .width_request(360)
        .height_request(400)
        .build();

    // ---------- barre d'en-tête ----------
    let header = adw::HeaderBar::new();
    let title = adw::WindowTitle::new("OpenHTPC Renamer", "Aucune médiathèque ouverte");
    header.set_title_widget(Some(&title));
    let open_btn = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text("Ouvrir une médiathèque (Ctrl+O)")
        .action_name("app.open")
        .build();
    header.pack_start(&open_btn);
    let reanalyze_btn = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Relancer l'analyse (F5)")
        .action_name("app.rescan")
        .build();
    header.pack_start(&reanalyze_btn);

    let menu = gio::Menu::new();
    let sec = gio::Menu::new();
    sec.append(Some("Valider tous les identifiés"), Some("app.confirm-all"));
    sec.append(Some("Annuler le dernier renommage…"), Some("app.undo"));
    menu.append_section(None, &sec);
    let sec2 = gio::Menu::new();
    sec2.append(Some("Préférences"), Some("app.prefs"));
    sec2.append(Some("À propos"), Some("app.about"));
    menu.append_section(None, &sec2);
    let menu_btn = gtk::MenuButton::builder().icon_name("open-menu-symbolic").menu_model(&menu).primary(true).build();
    header.pack_end(&menu_btn);

    // ---------- progression ----------
    let progress = gtk::ProgressBar::builder().show_text(true).margin_start(12).margin_end(12).margin_top(6).margin_bottom(6).build();
    let progress_rev = gtk::Revealer::builder().child(&progress).reveal_child(false).build();

    // ---------- liste ----------
    let search = gtk::SearchEntry::builder().placeholder_text("Filtrer par nom…").hexpand(true).build();
    let filter_dd = gtk::DropDown::from_strings(&FILTERS.iter().map(|f| f.0).collect::<Vec<_>>());
    let list_tools = gtk::Box::builder().spacing(6).margin_start(12).margin_end(12).margin_top(6).margin_bottom(6).build();
    list_tools.append(&search);
    list_tools.append(&filter_dd);
    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).build();
    list.add_css_class("navigation-sidebar");
    let list_scroll = gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    let list_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list_box.append(&list_tools);
    list_box.append(&list_scroll);
    list_box.set_width_request(420);

    // ---------- détails ----------
    let d_poster = gtk::Picture::builder().width_request(120).height_request(180).content_fit(gtk::ContentFit::Cover).build();
    d_poster.add_css_class("poster");
    let d_name = gtk::Label::builder().xalign(0.0).wrap(true).wrap_mode(gtk::pango::WrapMode::WordChar).selectable(true).build();
    d_name.add_css_class("title-2");
    let d_path = gtk::Label::builder().xalign(0.0).wrap(true).wrap_mode(gtk::pango::WrapMode::Char).selectable(true).build();
    d_path.add_css_class("dim-label");
    d_path.add_css_class("caption");
    let d_chips = gtk::FlowBox::builder().selection_mode(gtk::SelectionMode::None).max_children_per_line(12).row_spacing(6).column_spacing(6).build();
    let head_text = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8).hexpand(true).valign(gtk::Align::Center).build();
    head_text.append(&d_name);
    head_text.append(&d_path);
    head_text.append(&d_chips);
    let head = gtk::Box::builder().spacing(18).build();
    head.append(&d_poster);
    head.append(&head_text);

    let d_target = gtk::Label::builder().xalign(0.0).wrap(true).wrap_mode(gtk::pango::WrapMode::Char).selectable(true).build();
    d_target.add_css_class("newname");
    let target_title = gtk::Label::builder().label("Nouvel emplacement").xalign(0.0).build();
    target_title.add_css_class("heading");
    let target_box = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(6).build();
    target_box.add_css_class("target-box");
    target_box.append(&target_title);
    target_box.append(&d_target);
    let d_include = adw::SwitchRow::builder().title("Inclure dans le renommage").build();
    let inc_group = adw::PreferencesGroup::new();
    inc_group.add(&d_include);

    let d_query = adw::EntryRow::builder().title("Titre recherché").show_apply_button(true).build();
    let d_year = adw::EntryRow::builder().title("Année").show_apply_button(true).build();
    d_year.set_input_purpose(gtk::InputPurpose::Digits);
    let d_kind = adw::ComboRow::builder().title("Type").model(&gtk::StringList::new(&["Film", "Série"])).build();
    let d_id = adw::EntryRow::builder().title("Ou identifiant TMDB direct").show_apply_button(true).build();
    d_id.set_input_purpose(gtk::InputPurpose::Digits);
    let search_btn = gtk::Button::builder().icon_name("system-search-symbolic").tooltip_text("Rechercher sur TMDB").valign(gtk::Align::Center).build();
    search_btn.add_css_class("flat");
    let id_group = adw::PreferencesGroup::builder().title("Identification TMDB").description("Corrigez le titre ou l'année puis lancez la recherche. Un clic sur un résultat le valide.").build();
    id_group.set_header_suffix(Some(&search_btn));
    id_group.add(&d_kind);
    id_group.add(&d_query);
    id_group.add(&d_year);
    id_group.add(&d_id);

    let d_cands = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).build();
    d_cands.add_css_class("boxed-list");
    let d_cands_status = gtk::Label::builder().xalign(0.0).wrap(true).build();
    d_cands_status.add_css_class("dim-label");
    let cands_group = adw::PreferencesGroup::builder().title("Résultats").build();
    cands_group.add(&d_cands_status);
    cands_group.add(&d_cands);

    let d_tech = adw::PreferencesGroup::builder().title("Informations techniques").build();

    let details_content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(24).margin_top(24).margin_bottom(24).margin_start(12).margin_end(12).build();
    details_content.append(&head);
    details_content.append(&target_box);
    details_content.append(&inc_group);
    details_content.append(&id_group);
    details_content.append(&cands_group);
    details_content.append(&d_tech);
    let clamp = adw::Clamp::builder().maximum_size(820).child(&details_content).build();
    let details_scroll = gtk::ScrolledWindow::builder().child(&clamp).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).hexpand(true).build();
    let details_empty = adw::StatusPage::builder().icon_name("view-list-symbolic").title("Sélectionnez un média").description("Ses informations techniques et les correspondances TMDB s'afficheront ici.").build();
    let details_stack = gtk::Stack::new();
    details_stack.add_named(&details_empty, Some("empty"));
    details_stack.add_named(&details_scroll, Some("details"));

    let paned = gtk::Paned::builder().orientation(gtk::Orientation::Horizontal).start_child(&list_box).end_child(&details_stack).position(520).shrink_start_child(false).shrink_end_child(false).build();

    // ---------- page d'accueil ----------
    let open_big = gtk::Button::builder().label("Ouvrir la médiathèque…").halign(gtk::Align::Center).action_name("app.open").build();
    open_big.add_css_class("pill");
    open_big.add_css_class("suggested-action");
    let welcome = adw::StatusPage::builder()
        .icon_name("folder-videos-symbolic")
        .title("OpenHTPC Renamer")
        .description("Choisissez le dossier de vos films et séries (y compris un partage SMB monté).\nRien n'est renommé sans votre validation, et chaque opération peut être annulée.")
        .child(&open_big)
        .build();
    let stack = gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).build();
    stack.add_named(&welcome, Some("welcome"));
    stack.add_named(&paned, Some("main"));

    // ---------- barre du bas ----------
    let summary = gtk::Label::builder().xalign(0.0).build();
    let rename_btn = gtk::Button::builder().label("Renommer…").action_name("app.rename").build();
    rename_btn.add_css_class("suggested-action");
    let action_bar = gtk::ActionBar::new();
    action_bar.pack_start(&summary);
    action_bar.pack_end(&rename_btn);
    action_bar.set_revealed(false);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&progress_rev);
    content.append(&stack);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    toolbar.add_bottom_bar(&action_bar);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&toolbar));
    window.set_content(Some(&toasts));

    let ui: U = Rc::new(Ui {
        window: window.clone(),
        toasts,
        title,
        stack,
        list: list.clone(),
        rows: RefCell::new(vec![]),
        progress,
        progress_rev,
        summary,
        action_bar: action_bar.clone(),
        rename_btn,
        reanalyze_btn,
        details_stack,
        d_poster,
        d_name,
        d_path,
        d_chips,
        d_tech,
        d_tech_rows: RefCell::new(vec![]),
        d_query: d_query.clone(),
        d_year: d_year.clone(),
        d_kind: d_kind.clone(),
        d_id: d_id.clone(),
        d_cands,
        d_cands_status,
        d_target,
        d_include: d_include.clone(),
        state: RefCell::new(State {
            cfg,
            api,
            root: None,
            items: vec![],
            selected: None,
            filter: Filter::All,
            search: String::new(),
            busy: false,
            posters: HashMap::new(),
        }),
        updating: Cell::new(false),
    });

    // ---------- filtres ----------
    {
        let u = ui.clone();
        list.set_filter_func(move |row| {
            let st = u.state.borrow();
            let Some(it) = st.items.get(row.index() as usize) else { return false };
            let pass = match st.filter {
                Filter::All => true,
                Filter::Review => it.status == Status::Review,
                Filter::Ready => it.status.ready(),
                Filter::NotFound => matches!(it.status, Status::NotFound | Status::Error | Status::Locked),
                Filter::Done => matches!(it.status, Status::UpToDate | Status::Done),
            };
            pass && (st.search.is_empty() || it.path.to_string_lossy().to_lowercase().contains(&st.search))
        });
    }
    {
        let u = ui.clone();
        search.connect_search_changed(move |e| {
            u.state.borrow_mut().search = e.text().to_lowercase();
            u.list.invalidate_filter();
        });
    }
    {
        let u = ui.clone();
        filter_dd.connect_selected_notify(move |d| {
            u.state.borrow_mut().filter = FILTERS[d.selected() as usize].1;
            u.list.invalidate_filter();
        });
    }
    {
        let u = ui.clone();
        list.connect_row_selected(move |_, row| {
            let idx = row.map(|r| r.index() as usize);
            u.state.borrow_mut().selected = idx;
            show_details(&u);
        });
    }

    // ---------- détails : actions ----------
    {
        let u = ui.clone();
        search_btn.connect_clicked(move |_| run_search(&u));
    }
    for e in [&d_query, &d_year] {
        let u = ui.clone();
        e.connect_apply(move |_| run_search(&u));
        let u = ui.clone();
        e.connect_entry_activated(move |_| run_search(&u));
    }
    {
        let u = ui.clone();
        d_id.connect_apply(move |e| use_tmdb_id(&u, e.text().trim()));
        let u = ui.clone();
        d_id.connect_entry_activated(move |e| use_tmdb_id(&u, e.text().trim()));
    }
    {
        let u = ui.clone();
        d_kind.connect_selected_notify(move |c| {
            if u.is_updating() {
                return;
            }
            let kind = if c.selected() == 1 { MediaKind::Episode } else { MediaKind::Movie };
            let Some(i) = u.state.borrow().selected else { return };
            let changed = {
                let mut st = u.state.borrow_mut();
                let it = &mut st.items[i];
                if it.kind != kind {
                    it.kind = kind;
                    if kind == MediaKind::Episode && it.guess.season.is_none() {
                        it.guess.season = Some(1);
                    }
                    it.matched = None;
                    it.candidates.clear();
                    it.status = Status::Review;
                    engine::lock_status(it);
                    true
                } else {
                    false
                }
            };
            if changed {
                update_row(&u, i);
                show_details(&u);
                run_search(&u);
            }
        });
    }
    {
        let u = ui.clone();
        d_include.connect_active_notify(move |s| {
            if u.is_updating() {
                return;
            }
            let Some(i) = u.state.borrow().selected else { return };
            let changed = {
                let mut st = u.state.borrow_mut();
                let c = st.items[i].included != s.is_active();
                st.items[i].included = s.is_active();
                c
            };
            if changed {
                update_row(&u, i);
                update_summary(&u);
            }
        });
    }

    install_actions(app, &ui);

    // réouvrir le dernier dossier
    let last = ui.state.borrow().cfg.last_root.clone();
    if let Some(r) = last.filter(|r| r.is_dir()) {
        open_root(&ui, r);
    } else {
        update_summary(&ui);
    }
    window.present();
    if ui.state.borrow().cfg.tmdb_key.is_empty() {
        toast(&ui, "Renseignez votre clé API TMDB dans les Préférences");
    }
}

pub fn toast(ui: &Ui, msg: &str) {
    let t = adw::Toast::new(msg);
    t.set_timeout(4);
    ui.toasts.add_toast(t);
}

fn install_actions(app: &adw::Application, ui: &U) {
    let add = |name: &str, accels: &[&str], f: Box<dyn Fn()>| {
        let a = gio::SimpleAction::new(name, None);
        a.connect_activate(move |_, _| f());
        app.add_action(&a);
        if !accels.is_empty() {
            app.set_accels_for_action(&format!("app.{name}"), accels);
        }
    };
    let u = ui.clone();
    add("open", &["<Control>o"], Box::new(move || choose_folder(&u)));
    let u = ui.clone();
    add("rescan", &["F5"], Box::new(move || {
        let r = u.state.borrow().root.clone();
        if let Some(r) = r {
            open_root(&u, r);
        }
    }));
    let u = ui.clone();
    add("rename", &["<Control>r"], Box::new(move || rename_dialog(&u)));
    let u = ui.clone();
    add("undo", &["<Control>z"], Box::new(move || undo_dialog(&u)));
    let u = ui.clone();
    add("confirm-all", &[], Box::new(move || {
        let n = {
            let mut st = u.state.borrow_mut();
            let mut n = 0;
            for it in st.items.iter_mut().filter(|i| i.status == Status::Auto) {
                it.status = Status::Confirmed;
                n += 1;
            }
            n
        };
        refresh_all_rows(&u);
        toast(&u, &format!("{n} éléments validés"));
    }));
    let u = ui.clone();
    add("prefs", &["<Control>comma"], Box::new(move || super::prefs::show(&u)));
    let u = ui.clone();
    add("about", &[], Box::new(move || {
        let d = adw::AboutDialog::builder()
            .application_name("OpenHTPC Renamer")
            .application_icon("folder-videos-symbolic")
            .version(env!("CARGO_PKG_VERSION"))
            .comments("Renommage de films, séries, DVD, Blu-ray et UHD avec identification TMDB, pour OpenHTPC.\nCe produit utilise l'API TMDB mais n'est ni approuvé ni certifié par TMDB.")
            .license_type(gtk::License::MitX11)
            .build();
        d.present(Some(&u.window));
    }));
    let u = ui.clone();
    add("quit", &["<Control>q"], Box::new(move || u.window.close()));
}

fn choose_folder(ui: &U) {
    let dialog = gtk::FileDialog::builder().title("Choisir la médiathèque").modal(true).build();
    if let Some(r) = ui.state.borrow().cfg.last_root.as_ref() {
        dialog.set_initial_folder(Some(&gio::File::for_path(r)));
    }
    let u = ui.clone();
    dialog.select_folder(Some(&ui.window), gio::Cancellable::NONE, move |res| {
        if let Ok(f) = res {
            match f.path() {
                Some(p) => open_root(&u, p),
                None => toast(&u, "Ce dossier n'a pas de chemin local : montez le partage SMB (voir README)"),
            }
        }
    });
}

pub fn set_busy(ui: &Ui, busy: bool) {
    ui.state.borrow_mut().busy = busy;
    ui.progress_rev.set_reveal_child(busy);
    ui.rename_btn.set_sensitive(!busy);
    ui.reanalyze_btn.set_sensitive(!busy);
}

fn open_root(ui: &U, root: PathBuf) {
    if ui.state.borrow().busy {
        return;
    }
    {
        let mut st = ui.state.borrow_mut();
        st.root = Some(root.clone());
        st.cfg.last_root = Some(root.clone());
        let _ = st.cfg.save();
        st.items.clear();
        st.selected = None;
    }
    ui.title.set_subtitle(&root.display().to_string());
    ui.stack.set_visible_child_name("main");
    ui.action_bar.set_revealed(true);
    ui.details_stack.set_visible_child_name("empty");
    clear_box(&ui.list);
    ui.rows.borrow_mut().clear();
    set_busy(ui, true);
    ui.progress.set_fraction(0.0);
    ui.progress.set_text(Some("Parcours des dossiers…"));
    ui.progress.pulse();

    let (cfg, api) = {
        let st = ui.state.borrow();
        (st.cfg.clone(), st.api.clone())
    };
    let (tx, rx) = async_channel::unbounded::<Msg>();
    std::thread::spawn(move || {
        let mut items = scan::scan(&root);
        let _ = tx.send_blocking(Msg::Scanned(items.clone()));
        let rep = |p: Progress| {
            let _ = tx.send_blocking(Msg::P(p));
        };
        engine::probe_all(&mut items, cfg.probe_threads, &rep);
        let _ = tx.send_blocking(Msg::Probed(items.clone()));
        if api.has_key() {
            engine::identify_all(&mut items, &api, &cfg, &rep);
        } else {
            for (i, it) in items.iter_mut().enumerate() {
                it.status = Status::Review;
                it.message = Some("Clé API TMDB manquante".into());
                engine::lock_status(it);
                rep(Progress::Item(i, Box::new(it.clone())));
            }
        }
        let _ = tx.send_blocking(Msg::Finished);
    });

    let u = ui.clone();
    glib::spawn_future_local(async move {
        let mut phase_probe = true;
        while let Ok(msg) = rx.recv().await {
            match msg {
                Msg::Scanned(items) => {
                    let n = items.len();
                    u.state.borrow_mut().items = items;
                    build_rows(&u);
                    u.progress.set_text(Some(&format!("{n} médias trouvés — analyse technique…")));
                }
                Msg::Probed(items) => {
                    phase_probe = false;
                    // conserver les choix faits entre-temps
                    let mut st = u.state.borrow_mut();
                    for (dst, src) in st.items.iter_mut().zip(items) {
                        dst.tech = src.tech;
                    }
                    drop(st);
                    refresh_all_rows(&u);
                    show_details(&u);
                }
                Msg::P(Progress::Step { done, total, label }) => {
                    let frac = if total == 0 { 1.0 } else { done as f64 / total as f64 };
                    let prefix = if phase_probe { "1/2" } else { "2/2" };
                    u.progress.set_fraction(frac);
                    u.progress.set_text(Some(&format!("{prefix} · {label} ({done}/{total})")));
                }
                Msg::P(Progress::Item(i, it)) => {
                    {
                        let mut st = u.state.borrow_mut();
                        if let Some(dst) = st.items.get_mut(i) {
                            if !matches!(dst.status, Status::Confirmed) || dst.matched.is_none() {
                                let included = dst.included;
                                *dst = *it;
                                dst.included = included && dst.locked.is_none();
                            }
                        }
                    }
                    update_row(&u, i);
                    if u.state.borrow().selected == Some(i) {
                        show_details(&u);
                    }
                }
                Msg::Finished => break,
            }
        }
        set_busy(&u, false);
        update_summary(&u);
        u.list.invalidate_filter();
        let (n, rev) = {
            let st = u.state.borrow();
            (st.items.len(), st.items.iter().filter(|i| i.status == Status::Review).count())
        };
        toast(&u, &format!("Analyse terminée : {n} médias, {rev} à vérifier"));
    });
}

enum Msg {
    Scanned(Vec<MediaItem>),
    Probed(Vec<MediaItem>),
    P(Progress),
    Finished,
}

fn build_rows(ui: &U) {
    clear_box(&ui.list);
    let mut rows = vec![];
    let n = ui.state.borrow().items.len();
    for _ in 0..n {
        let icon = gtk::Image::new();
        icon.set_pixel_size(24);
        let name = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::Middle).build();
        name.add_css_class("oldname");
        let target = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::Middle).build();
        target.add_css_class("heading");
        let status = gtk::Label::builder().valign(gtk::Align::Center).build();
        status.add_css_class("status-pill");
        let check = gtk::CheckButton::builder().valign(gtk::Align::Center).tooltip_text("Inclure dans le renommage").build();
        let texts = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).hexpand(true).build();
        texts.append(&target);
        texts.append(&name);
        let b = gtk::Box::builder().spacing(10).margin_top(6).margin_bottom(6).build();
        b.append(&check);
        b.append(&icon);
        b.append(&texts);
        b.append(&status);
        let row = gtk::ListBoxRow::builder().child(&b).build();
        ui.list.append(&row);
        rows.push(RowW { name, target, status, icon, check });
    }
    *ui.rows.borrow_mut() = rows;
    for i in 0..n {
        update_row(ui, i);
    }
    // branchement des cases à cocher
    let rows = ui.rows.borrow();
    for (i, r) in rows.iter().enumerate() {
        let w = Rc::downgrade(ui);
        r.check.connect_toggled(move |c| {
            if let Some(u) = w.upgrade() {
                let u = &u;
                if u.is_updating() {
                    return;
                }
                let changed = {
                    let mut st = u.state.borrow_mut();
                    let ch = st.items[i].included != c.is_active();
                    st.items[i].included = c.is_active();
                    ch
                };
                if changed {
                    update_summary(u);
                    if u.state.borrow().selected == Some(i) {
                        let _g = u.guard();
                        u.d_include.set_active(c.is_active());
                    }
                }
            }
        });
    }
}

fn update_row(ui: &Ui, i: usize) {
    let _g = ui.guard();
    let st = ui.state.borrow();
    let rows = ui.rows.borrow();
    let (Some(it), Some(r)) = (st.items.get(i), rows.get(i)) else { return };
    r.icon.set_icon_name(Some(icon_for(it)));
    r.name.set_label(&rel(&it.path, &st.root));
    r.name.set_tooltip_text(Some(&it.path.display().to_string()));
    let new = match &it.matched {
        Some(m) => {
            let y = m.year.map(|y| format!(" ({y})")).unwrap_or_default();
            match it.kind {
                MediaKind::Episode => format!(
                    "{}{} · S{:02}E{:02}",
                    m.title,
                    y,
                    it.guess.season.unwrap_or(0),
                    it.guess.episodes.first().copied().unwrap_or(0)
                ),
                MediaKind::Movie => format!("{}{}", m.title, y),
            }
        }
        None => format!("« {} »{}", it.guess.title, it.guess.year.map(|y| format!(" ({y})")).unwrap_or_default()),
    };
    r.target.set_label(&new);
    r.status.set_label(it.status.label());
    for c in ["success", "warning", "error", "accent", "dim-label"] {
        r.status.remove_css_class(c);
    }
    r.status.add_css_class(it.status.css());
    if r.check.is_active() != it.included {
        r.check.set_active(it.included);
    }
    r.check.set_sensitive(it.status.ready());
}

fn refresh_all_rows(ui: &U) {
    let n = ui.state.borrow().items.len();
    for i in 0..n {
        update_row(ui, i);
    }
    update_summary(ui);
    ui.list.invalidate_filter();
    show_details(ui);
}

fn update_summary(ui: &Ui) {
    let st = ui.state.borrow();
    let ready = st.items.iter().filter(|i| i.status.ready() && i.included).count();
    let review = st.items.iter().filter(|i| i.status == Status::Review).count();
    let nf = st.items.iter().filter(|i| matches!(i.status, Status::NotFound | Status::Error)).count();
    let ok = st.items.iter().filter(|i| matches!(i.status, Status::UpToDate | Status::Done)).count();
    ui.summary.set_markup(&format!(
        "<b>{}</b> médias · <b>{ready}</b> prêts · <span foreground='#c88800'><b>{review}</b> à vérifier</span> · <b>{nf}</b> non trouvés · <b>{ok}</b> conformes",
        st.items.len()
    ));
    ui.rename_btn.set_label(&format!("Renommer {ready} éléments…"));
    ui.rename_btn.set_sensitive(ready > 0 && !st.busy);
}

fn show_details(ui: &U) {
    let _g = ui.guard();
    let st = ui.state.borrow();
    let Some(i) = st.selected else {
        ui.details_stack.set_visible_child_name("empty");
        return;
    };
    let Some(it) = st.items.get(i) else { return };
    ui.details_stack.set_visible_child_name("details");

    ui.d_name.set_label(&it.display_name());
    ui.d_path.set_label(&format!("{} · {}", it.layout.label(), it.path.display()));

    // puces techniques
    while let Some(c) = ui.d_chips.first_child() {
        ui.d_chips.remove(&c);
    }
    let t = &it.tech;
    for s in [&t.source, &t.resolution, &t.codec].into_iter().flatten() {
        ui.d_chips.insert(&chip(s, None), -1);
    }
    if let Some(b) = t.bit_depth.filter(|b| *b > 8) {
        ui.d_chips.insert(&chip(&format!("{b} bits"), None), -1);
    }
    for h in &t.hdr {
        ui.d_chips.insert(&chip(h, Some("hdr")), -1);
    }
    let a = t.audio_label();
    if !a.is_empty() {
        ui.d_chips.insert(&chip(&a, Some("audio")), -1);
    }
    if let Some(l) = &t.langs {
        ui.d_chips.insert(&chip(l, None), -1);
    }

    // détail technique
    for r in ui.d_tech_rows.borrow_mut().drain(..) {
        ui.d_tech.remove(&r);
    }
    let add_row = |title: &str, sub: String| {
        let r = adw::ActionRow::builder().title(title).subtitle(glib::markup_escape_text(&sub)).subtitle_selectable(true).build();
        r.add_css_class("property");
        ui.d_tech.add(&r);
        ui.d_tech_rows.borrow_mut().push(r);
    };
    let analysis = if t.probed { "ffprobe (analyse réelle)".to_string() } else { "déduites du nom de fichier".to_string() };
    add_row("Source des informations", analysis);
    if let Some(e) = &t.probe_error {
        add_row("Remarque", e.clone());
    }
    let video = [t.resolution.clone(), t.codec.clone(), t.bit_depth.map(|b| format!("{b} bits")), (!t.hdr.is_empty()).then(|| t.hdr.join(" + "))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    if !video.is_empty() {
        add_row("Vidéo", video);
    }
    for (n, a) in t.audio.iter().enumerate() {
        let lang = a.lang.as_deref().map(|l| format!(" · {}", l.to_uppercase())).unwrap_or_default();
        add_row(&format!("Audio {}", n + 1), format!("{} {}{lang}", a.format, a.channels));
    }
    if let Some(d) = t.duration_min {
        add_row("Durée", format!("{} h {:02} min", d / 60, d % 60));
    }

    // affiche
    let poster = it.matched.as_ref().and_then(|m| m.poster_path.clone());
    match poster.as_ref().and_then(|p| st.posters.get(p)) {
        Some(tex) => ui.d_poster.set_paintable(Some(tex)),
        None => ui.d_poster.set_paintable(gdk::Paintable::NONE),
    }

    // cible
    let target = if let Some(l) = &it.locked {
        format!("⛔ Non renommable\n{l}")
    } else if it.matched.is_some() {
        rel(&naming::target_path(it, &st.cfg), &st.root)
    } else {
        "— choisissez une correspondance TMDB —".into()
    };
    ui.d_target.set_label(&target);
    if it.locked.is_some() {
        ui.d_target.add_css_class("error");
        ui.d_target.remove_css_class("newname");
    } else {
        ui.d_target.remove_css_class("error");
        ui.d_target.add_css_class("newname");
    }
    ui.d_include.set_active(it.included);
    ui.d_include.set_sensitive(it.status.ready());

    // identification
    ui.d_query.set_text(&it.guess.title);
    ui.d_year.set_text(&it.guess.year.map(|y| y.to_string()).unwrap_or_default());
    ui.d_kind.set_selected(if it.kind == MediaKind::Episode { 1 } else { 0 });
    ui.d_id.set_text(&it.matched.as_ref().map(|m| m.id.to_string()).unwrap_or_default());

    // candidats
    clear_box(&ui.d_cands);
    let msg = match (&it.message, it.candidates.is_empty(), it.status) {
        (Some(m), _, _) => m.clone(),
        (None, true, Status::Pending) => "Recherche en cours…".into(),
        (None, true, _) => "Aucun résultat : modifiez le titre ou l'année, ou saisissez l'identifiant TMDB.".into(),
        (None, false, _) => format!("{} résultat(s). Cliquez sur le bon pour le valider.", it.candidates.len()),
    };
    ui.d_cands_status.set_label(&msg);
    let mut cands = it.candidates.clone();
    if let Some(m) = &it.matched {
        if !cands.iter().any(|c| c.id == m.id) {
            cands.insert(0, m.clone());
        }
    }
    let mut missing_posters = vec![];
    for c in &cands {
        let selected = it.matched.as_ref().map(|m| m.id == c.id).unwrap_or(false);
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&format!("{}{}", c.title, c.year.map(|y| format!(" ({y})")).unwrap_or_default())))
            .subtitle(glib::markup_escape_text(&{
                let mut s = String::new();
                if c.original_title != c.title {
                    s.push_str(&format!("{} · ", c.original_title));
                }
                s.push_str(&format!("{} · tmdb {}", c.original_language.to_uppercase(), c.id));
                if !c.overview.is_empty() {
                    let ov: String = c.overview.chars().take(160).collect();
                    let more = if c.overview.chars().count() > 160 { "…" } else { "" };
                    s.push_str(&format!("\n{ov}{more}"));
                }
                s
            }))
            .subtitle_lines(3)
            .activatable(true)
            .build();
        let pic = gtk::Picture::builder().width_request(46).height_request(69).content_fit(gtk::ContentFit::Cover).build();
        pic.add_css_class("poster");
        if let Some(p) = &c.poster_path {
            match st.posters.get(p) {
                Some(tex) => pic.set_paintable(Some(tex)),
                None => missing_posters.push(p.clone()),
            }
        }
        row.add_prefix(&pic);
        if selected {
            let ic = gtk::Image::from_icon_name("object-select-symbolic");
            ic.add_css_class("success");
            row.add_suffix(&ic);
        }
        let w = Rc::downgrade(ui);
        let cc = c.clone();
        row.connect_activated(move |_| {
            if let Some(u) = w.upgrade() {
                choose_match(&u, cc.clone());
            }
        });
        ui.d_cands.append(&row);
    }
    if let Some(p) = &poster {
        if !st.posters.contains_key(p) {
            missing_posters.push(p.clone());
        }
    }
    let api = st.api.clone();
    drop(st);
    if !missing_posters.is_empty() {
        load_posters(ui, api, missing_posters);
    }
}

fn load_posters(ui: &U, api: Arc<Tmdb>, paths: Vec<String>) {
    let w = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let res = gio::spawn_blocking(move || {
            paths.into_iter().filter_map(|p| api.poster(&p).map(|b| (p, b))).collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let Some(u) = w.upgrade() else { return };
        let u = &u;
        let mut any = false;
        {
            let mut st = u.state.borrow_mut();
            for (p, bytes) in res {
                if let Ok(t) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)) {
                    st.posters.insert(p, t);
                    any = true;
                }
            }
        }
        if any {
            show_details(u);
        }
    });
}

fn group_of(st: &State, i: usize) -> Vec<usize> {
    let key = st.items[i].group_key();
    st.items.iter().enumerate().filter(|(_, it)| it.group_key() == key).map(|(j, _)| j).collect()
}

/// L'utilisateur valide un candidat : appliqué à tout le groupe (tous les épisodes de la série).
fn choose_match(ui: &U, m: TmdbMatch) {
    let Some(i) = ui.state.borrow().selected else { return };
    let (idxs, items, api, cfg) = {
        let st = ui.state.borrow();
        let idxs = group_of(&st, i);
        let items: Vec<MediaItem> = idxs.iter().map(|&j| st.items[j].clone()).collect();
        (idxs, items, st.api.clone(), st.cfg.clone())
    };
    let u = ui.clone();
    glib::spawn_future_local(async move {
        let res = gio::spawn_blocking(move || {
            items
                .into_iter()
                .map(|mut it| {
                    it.message = None;
                    engine::apply_match(&mut it, m.clone(), &api, Status::Confirmed, &cfg);
                    it.included = true;
                    engine::lock_status(&mut it);
                    it
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        {
            let mut st = u.state.borrow_mut();
            for (j, it) in idxs.iter().zip(res) {
                st.items[*j] = it;
            }
        }
        for j in &idxs {
            update_row(&u, *j);
        }
        update_summary(&u);
        show_details(&u);
        if idxs.len() > 1 {
            toast(&u, &format!("Appliqué aux {} épisodes de la série", idxs.len()));
        }
    });
}

fn run_search(ui: &U) {
    let Some(i) = ui.state.borrow().selected else { return };
    let query = ui.d_query.text().trim().to_string();
    let year: Option<u16> = ui.d_year.text().trim().parse().ok();
    if query.is_empty() {
        return;
    }
    let (kind, api, idxs) = {
        let mut st = ui.state.borrow_mut();
        let idxs = group_of(&st, i);
        for &j in &idxs {
            st.items[j].guess.title = query.clone();
            st.items[j].guess.year = year;
            st.items[j].message = Some("Recherche…".into());
        }
        (st.items[i].kind, st.api.clone(), idxs)
    };
    ui.d_cands_status.set_label("Recherche…");
    let u = ui.clone();
    glib::spawn_future_local(async move {
        let q = query.clone();
        let res = gio::spawn_blocking(move || api.search(kind, &q, year)).await.unwrap_or_else(|_| Err("erreur interne".into()));
        {
            let mut st = u.state.borrow_mut();
            for &j in &idxs {
                let it = &mut st.items[j];
                match &res {
                    Ok(c) => {
                        it.candidates = c.clone();
                        it.message = None;
                        if it.status != Status::Confirmed {
                            it.status = if c.is_empty() { Status::NotFound } else { Status::Review };
                            if let Some(k) = tmdb::auto_pick(c, &query, year) {
                                // pré-sélection, mais l'utilisateur doit confirmer
                                it.matched = Some(c[k].clone());
                            }
                        }
                    }
                    Err(e) => it.message = Some(e.clone()),
                }
                engine::lock_status(it);
            }
        }
        for &j in &idxs {
            update_row(&u, j);
        }
        update_summary(&u);
        show_details(&u);
    });
}

fn use_tmdb_id(ui: &U, text: &str) {
    let Ok(id) = text.parse::<u64>() else {
        toast(ui, "Identifiant TMDB invalide");
        return;
    };
    let Some(i) = ui.state.borrow().selected else { return };
    if ui.state.borrow().items[i].matched.as_ref().map(|m| m.id) == Some(id) {
        return;
    }
    let (kind, api) = {
        let st = ui.state.borrow();
        (st.items[i].kind, st.api.clone())
    };
    let u = ui.clone();
    glib::spawn_future_local(async move {
        match gio::spawn_blocking(move || api.by_id(kind, id)).await {
            Ok(Ok(m)) => choose_match(&u, m),
            Ok(Err(e)) => toast(&u, &e),
            Err(_) => {}
        }
    });
}

// ---------------- renommage ----------------

fn rename_dialog(ui: &U) {
    let (plans, root) = {
        let st = ui.state.borrow();
        let idx: Vec<usize> = st
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.included && it.status.ready())
            .map(|(i, _)| i)
            .collect();
        (naming::build_plan(&st.items, &idx, &st.cfg), st.root.clone())
    };
    let plans: Vec<_> = plans.into_iter().filter(|p| !p.ops.is_empty()).collect();
    if plans.is_empty() {
        toast(ui, "Rien à renommer : tout est déjà conforme");
        return;
    }
    let Some(root) = root else { return };

    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).build();
    list.add_css_class("boxed-list");
    {
        let st = ui.state.borrow();
        for p in &plans {
            let it = &st.items[p.index];
            let old = gtk::Label::builder().label(format!("− {}", rel(&it.path, &st.root))).xalign(0.0).wrap(true).wrap_mode(gtk::pango::WrapMode::Char).build();
            old.add_css_class("error");
            old.add_css_class("newname");
            let new = gtk::Label::builder().label(format!("+ {}", rel(&p.target, &st.root))).xalign(0.0).wrap(true).wrap_mode(gtk::pango::WrapMode::Char).build();
            new.add_css_class("success");
            new.add_css_class("newname");
            let extra = p.ops.iter().filter(|o| matches!(o, naming::Op::Rename { .. })).count().saturating_sub(1);
            let b = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).margin_top(10).margin_bottom(10).margin_start(12).margin_end(12).build();
            b.append(&old);
            b.append(&new);
            if extra > 0 {
                let l = gtk::Label::builder().label(format!("+ {extra} fichier(s) annexe(s) ou dossier(s)")).xalign(0.0).build();
                l.add_css_class("dim-label");
                l.add_css_class("caption");
                b.append(&l);
            }
            list.append(&gtk::ListBoxRow::builder().child(&b).activatable(false).build());
        }
    }
    let scroll = gtk::ScrolledWindow::builder().child(&adw::Clamp::builder().maximum_size(1000).child(&list).margin_top(12).margin_bottom(12).margin_start(12).margin_end(12).build()).vexpand(true).build();
    let n = plans.len();
    let cancel = gtk::Button::with_label("Annuler");
    let go = gtk::Button::with_label(&format!("Renommer {n} éléments"));
    go.add_css_class("destructive-action");
    let header = adw::HeaderBar::builder().show_end_title_buttons(false).show_start_title_buttons(false).build();
    header.pack_start(&cancel);
    header.pack_end(&go);
    let info = gtk::Label::builder()
        .label("Vérifiez les changements. Un journal est enregistré : « Annuler le dernier renommage » remet tout en place.")
        .wrap(true)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .build();
    info.add_css_class("dim-label");
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&header);
    tv.add_top_bar(&info);
    tv.set_content(Some(&scroll));
    let dialog = adw::Dialog::builder().title("Aperçu du renommage").content_width(1000).content_height(700).child(&tv).build();
    {
        let d = dialog.clone();
        cancel.connect_clicked(move |_| {
            d.close();
        });
    }
    {
        let d = dialog.clone();
        let u = ui.clone();
        go.connect_clicked(move |_| {
            d.close();
            execute(&u, plans.clone(), root.clone());
        });
    }
    dialog.present(Some(&ui.window));
}

fn execute(ui: &U, plans: Vec<naming::ItemPlan>, root: PathBuf) {
    set_busy(ui, true);
    ui.progress.set_text(Some("Renommage en cours…"));
    ui.progress.pulse();
    let u = ui.clone();
    glib::spawn_future_local(async move {
        let out = gio::spawn_blocking(move || rename::apply(&plans, &root)).await;
        set_busy(&u, false);
        let Ok(out) = out else { return };
        {
            let mut st = u.state.borrow_mut();
            for (i, target) in &out.ok {
                let it = &mut st.items[*i];
                it.path = target.clone();
                it.status = Status::Done;
                it.message = None;
            }
            for (i, e) in &out.failed {
                st.items[*i].status = Status::Error;
                st.items[*i].message = Some(e.clone());
            }
        }
        refresh_all_rows(&u);
        if out.failed.is_empty() {
            toast(&u, &format!("{} éléments renommés", out.ok.len()));
        } else {
            let body = out.failed.iter().take(8).map(|(_, e)| format!("• {e}")).collect::<Vec<_>>().join("\n");
            let d = adw::AlertDialog::new(
                Some(&format!("{} réussis, {} en erreur", out.ok.len(), out.failed.len())),
                Some(&body),
            );
            d.add_response("ok", "Fermer");
            d.present(Some(&u.window));
        }
    });
}

fn undo_dialog(ui: &U) {
    let Some(j) = rename::last_journal() else {
        toast(ui, "Aucun renommage à annuler");
        return;
    };
    let name = j.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let d = adw::AlertDialog::new(
        Some("Annuler le dernier renommage ?"),
        Some(&format!("Les fichiers et dossiers renommés le {} retrouveront leur nom d'origine.", {
            let (d, h) = name.split_once('_').unwrap_or((&name, ""));
            format!("{d} à {}", h.replace('-', ":"))
        })),
    );
    d.add_response("cancel", "Garder");
    d.add_response("undo", "Annuler le renommage");
    d.set_response_appearance("undo", adw::ResponseAppearance::Destructive);
    d.set_default_response(Some("cancel"));
    let u = ui.clone();
    d.connect_response(None, move |_, r| {
        if r != "undo" {
            return;
        }
        let j = j.clone();
        let u = u.clone();
        glib::spawn_future_local(async move {
            match gio::spawn_blocking(move || rename::undo(&j)).await {
                Ok(Ok((n, errs))) => {
                    if errs.is_empty() {
                        toast(&u, &format!("{n} opérations annulées"));
                    } else {
                        toast(&u, &format!("{n} opérations annulées, {} erreurs", errs.len()));
                    }
                    let r = u.state.borrow().root.clone();
                    if let Some(r) = r {
                        open_root(&u, r);
                    }
                }
                Ok(Err(e)) => toast(&u, &e),
                Err(_) => {}
            }
        });
    });
    d.present(Some(&ui.window));
}

pub fn after_prefs(ui: &U) {
    {
        let mut st = ui.state.borrow_mut();
        st.api = Arc::new(Tmdb::new(&st.cfg.tmdb_key, &st.cfg.language));
        let _ = st.cfg.save();
        let cfg = st.cfg.clone();
        for it in st.items.iter_mut() {
            if it.status == Status::UpToDate {
                it.status = Status::Confirmed;
            }
            engine::refresh_status(it, &cfg);
        }
    }
    refresh_all_rows(ui);
}
