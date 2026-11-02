mod prefs;
mod window;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

const CSS: &str = r#"
.status-pill { border-radius: 999px; padding: 1px 10px; font-size: 0.8em; font-weight: 700; }
.status-pill.success { background: alpha(@success_color, 0.16); }
.status-pill.warning { background: alpha(@warning_color, 0.18); }
.status-pill.error   { background: alpha(@error_color, 0.16); }
.status-pill.accent  { background: alpha(@accent_color, 0.16); }
.status-pill.dim-label { background: alpha(@window_fg_color, 0.08); }
.chip { border-radius: 6px; padding: 2px 8px; font-size: 0.85em; font-weight: 600;
        background: alpha(@accent_bg_color, 0.14); color: @accent_color; }
.chip.hdr { background: alpha(#e5a50a, 0.18); color: #c88800; }
.chip.audio { background: alpha(#9141ac, 0.16); color: #9141ac; }
.oldname { font-size: 0.85em; }
.newname { font-family: monospace; font-size: 0.9em; }
.poster { border-radius: 8px; background: alpha(@window_fg_color, 0.06); }
.target-box { padding: 12px; border-radius: 12px; background: alpha(@accent_bg_color, 0.08); }
"#;

pub fn run(app_id: &str) -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(app_id)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(|_| {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(CSS);
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
    app.connect_activate(window::build);
    // on ne laisse pas GTK interpréter nos propres arguments
    app.run_with_args::<&str>(&[])
}
