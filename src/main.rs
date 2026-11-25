mod core;
mod models;
mod ui;

use adw::prelude::*;
use gtk::gdk;

const APP_ID: &str = "io.github.oparada1988.Ballista";

fn main() -> glib::ExitCode {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_startup(|_| {
        load_css();
    });

    app.connect_activate(|app| {
        let main_win = ui::MainWindow::new(app);
        main_win.present();
    });

    app.run()
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("ui/style.css"));

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }
}
