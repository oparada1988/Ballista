mod core;
mod models;
mod native_sdk;
mod plugins;
mod ui;

use adw::prelude::*;
use gtk::gdk;
use std::cell::RefCell;
use std::rc::Rc;

const APP_ID: &str = "io.github.oparada1988.Ballista";

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt::init();

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    app.connect_startup(|_| {
        load_css();
    });

    let main_win_cell: Rc<RefCell<Option<Rc<ui::MainWindow>>>> = Rc::new(RefCell::new(None));
    let hold_guard_cell: Rc<RefCell<Option<gio::ApplicationHoldGuard>>> = Rc::new(RefCell::new(None));

    // Handle single-instance CLI launches:
    //   `ballista --background` -> boots hardware + tray silently with no popup window
    //   `ballista`              -> opens window on first launch, or focuses existing hidden window in <5ms
    let win_cli = main_win_cell.clone();
    let hold_cli = hold_guard_cell.clone();
    app.connect_command_line(move |app, cmdline| {
        let start_background = cmdline.arguments().iter().any(|arg| {
            let s = arg.to_string_lossy();
            s == "--background" || s == "-b" || s == "--tray"
        });

        let mut slot = win_cli.borrow_mut();
        if let Some(ref existing) = *slot {
            if !start_background {
                existing.show_and_present();
            }
        } else {
            // Keep background service alive even when the window is closed/hidden
            *hold_cli.borrow_mut() = Some(app.hold());
            let main_win = Rc::new(ui::MainWindow::new(app, hold_cli.clone()));
            if !start_background {
                main_win.show_and_present();
            } else {
                tracing::info!("Ballista started in background tray service mode");
            }
            *slot = Some(main_win);
        }

        0.into()
    });

    // Fallback activation (e.g. D-Bus Activate without command line)
    let win_act = main_win_cell.clone();
    let hold_act = hold_guard_cell.clone();
    app.connect_activate(move |app| {
        let mut slot = win_act.borrow_mut();
        if let Some(ref existing) = *slot {
            existing.show_and_present();
        } else {
            *hold_act.borrow_mut() = Some(app.hold());
            let main_win = Rc::new(ui::MainWindow::new(app, hold_act.clone()));
            main_win.show_and_present();
            *slot = Some(main_win);
        }
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
