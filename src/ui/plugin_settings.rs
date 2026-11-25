#![allow(deprecated)]

use crate::core::plugin_bridge::BridgeConfigRow;
use crate::core::PluginManager;
use crate::ui::inspector::{hex_to_rgba, rgba_to_hex};
use adw::prelude::*;
use adw::{
    ActionRow, ComboRow, EntryRow, HeaderBar, PasswordEntryRow, PreferencesGroup, PreferencesPage,
    SpinRow, SwitchRow, ToolbarView, Window,
};
use gtk::{Box, Button, ColorDialog, ColorDialogButton, Label, Orientation, Scale, ScrolledWindow};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub type ActivePluginSettingsRegistry =
    Rc<RefCell<Option<(String, std::boxed::Box<dyn Fn(&[BridgeConfigRow])>)>>>;

pub fn open_plugin_settings_dialog(
    parent: &impl IsA<gtk::Window>,
    plugin_id: &str,
    plugin_name: &str,
    plugin_manager: Rc<RefCell<PluginManager>>,
    active_dialog_cb: ActivePluginSettingsRegistry,
) {
    let window = Window::builder()
        .title(&format!("{plugin_name} Settings"))
        .transient_for(parent)
        .modal(true)
        .default_width(600)
        .default_height(500)
        .build();

    window.add_css_class("ballista-modal");
    window.add_css_class("ballista-midnight-window");

    let toolbar_view = ToolbarView::new();

    let header = HeaderBar::new();
    header.add_css_class("ballista-headerbar");
    let close_btn = Button::with_label("Done");
    close_btn.add_css_class("suggested-action");
    let win_close_btn = window.clone();
    close_btn.connect_clicked(move |_| {
        win_close_btn.close();
    });
    header.pack_end(&close_btn);
    toolbar_view.add_top_bar(&header);

    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();

    let page = PreferencesPage::new();
    let group_holder = Box::new(Orientation::Vertical, 12);
    group_holder.set_margin_top(16);
    group_holder.set_margin_bottom(16);
    group_holder.set_margin_start(16);
    group_holder.set_margin_end(16);

    let loading_lbl = Label::new(Some("Loading plugin settings..."));
    loading_lbl.add_css_class("dim-label");
    loading_lbl.set_margin_top(24);
    group_holder.append(&loading_lbl);

    let custom_group = PreferencesGroup::new();
    custom_group.set_title(&format!("{plugin_name} Configuration"));
    custom_group.set_description(Some("Live settings provided natively by this StreamController plugin"));
    page.add(&custom_group);

    let content_wrapper = Box::new(Orientation::Vertical, 0);
    content_wrapper.append(&group_holder);
    scrolled.set_child(Some(&content_wrapper));
    toolbar_view.set_content(Some(&scrolled));
    window.set_content(Some(&toolbar_view));

    // Track currently rendered rows & widget handles for flicker-free in-place updates
    let rendered_rows: Rc<RefCell<Vec<BridgeConfigRow>>> = Rc::new(RefCell::new(Vec::new()));
    let rendered_widgets: Rc<RefCell<Vec<gtk::Widget>>> = Rc::new(RefCell::new(Vec::new()));
    let is_syncing: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    let pid_str = plugin_id.to_string();
    let pname_str = plugin_name.to_string();
    let pm_render = plugin_manager.clone();
    let holder_ref = group_holder.clone();
    let rendered_rows_ref = rendered_rows.clone();
    let rendered_widgets_ref = rendered_widgets.clone();
    let syncing_ref = is_syncing.clone();

    let render_rows = Rc::new(move |rows: &[BridgeConfigRow]| {
        let visible_rows: Vec<BridgeConfigRow> = rows.iter().filter(|r| r.visible).cloned().collect();

        // Check if we can update in-place without destroying focused text entries / buttons
        let can_patch = {
            let prev = rendered_rows_ref.borrow();
            let widgets = rendered_widgets_ref.borrow();
            prev.len() == visible_rows.len()
                && widgets.len() == visible_rows.len()
                && !visible_rows.is_empty()
                && prev.iter().zip(visible_rows.iter()).all(|(a, b)| {
                    a.index == b.index && a.row_type == b.row_type && a.title == b.title
                })
        };

        if can_patch {
            syncing_ref.set(true);
            let widgets = rendered_widgets_ref.borrow();
            for (row, widget) in visible_rows.iter().zip(widgets.iter()) {
                widget.set_sensitive(row.sensitive);
                match row.row_type.as_str() {
                    "EntryRow" => {
                        if let Some(er) = widget.downcast_ref::<EntryRow>() {
                            er.set_editable(row.editable);
                            if !er.has_focus() && er.text().as_str() != row.text {
                                er.set_text(&row.text);
                            }
                        }
                    }
                    "PasswordRow" => {
                        if let Some(pr) = widget.downcast_ref::<PasswordEntryRow>() {
                            pr.set_editable(row.editable);
                            if !pr.has_focus() && pr.text().as_str() != row.text {
                                pr.set_text(&row.text);
                            }
                        }
                    }
                    "SwitchRow" => {
                        if let Some(sr) = widget.downcast_ref::<SwitchRow>() {
                            if sr.is_active() != row.active {
                                sr.set_active(row.active);
                            }
                        }
                    }
                    "ComboRow" => {
                        if let Some(cr) = widget.downcast_ref::<ComboRow>() {
                            let prev_opts = rendered_rows_ref
                                .borrow()
                                .iter()
                                .find(|r| r.index == row.index)
                                .map(|r| r.options.clone())
                                .unwrap_or_default();
                            if prev_opts != row.options {
                                let str_opts: Vec<&str> = row.options.iter().map(|s| s.as_str()).collect();
                                let model = gtk::StringList::new(&str_opts);
                                cr.set_model(Some(&model));
                            }
                            if row.selected < row.options.len() && cr.selected() as usize != row.selected {
                                cr.set_selected(row.selected as u32);
                            }
                        }
                    }
                    "SpinRow" => {
                        if let Some(sp) = widget.downcast_ref::<SpinRow>() {
                            if !sp.has_focus() && (sp.value() - row.value).abs() > 1e-6 {
                                sp.set_value(row.value);
                            }
                        }
                    }
                    "ScaleRow" => {
                        if let Some(scale) = widget.downcast_ref::<Scale>() {
                            if (scale.value() - row.value).abs() > 1e-6 {
                                scale.set_value(row.value);
                            }
                        }
                    }
                    "ColorRow" => {
                        if let Some(cb) = widget.downcast_ref::<ColorDialogButton>() {
                            cb.set_rgba(&hex_to_rgba(&row.color));
                        }
                    }
                    "ButtonRow" => {
                        if let Some(btn) = widget.downcast_ref::<Button>() {
                            btn.set_sensitive(row.sensitive);
                            if !row.button_label.is_empty() {
                                btn.set_label(&row.button_label);
                            }
                            if row.suggested {
                                btn.add_css_class("suggested-action");
                            } else {
                                btn.remove_css_class("suggested-action");
                            }
                        }
                    }
                    "ActionRow" => {
                        if let Some(ar) = widget.downcast_ref::<ActionRow>() {
                            ar.set_subtitle(&glib::markup_escape_text(&row.subtitle));
                        }
                    }
                    _ => {}
                }
            }
            *rendered_rows_ref.borrow_mut() = visible_rows;
            syncing_ref.set(false);
            return;
        }

        // Full rebuild of PreferencesGroup
        while let Some(child) = holder_ref.first_child() {
            holder_ref.remove(&child);
        }
        rendered_widgets_ref.borrow_mut().clear();

        if visible_rows.is_empty() {
            let empty = Label::new(Some("This plugin does not expose any global configuration settings."));
            empty.add_css_class("dim-label");
            empty.set_margin_top(28);
            empty.set_wrap(true);
            holder_ref.append(&empty);
            *rendered_rows_ref.borrow_mut() = Vec::new();
            return;
        }

        let group = PreferencesGroup::new();
        group.set_title(&format!("{pname_str} Settings"));
        group.set_description(Some("Changes apply immediately to the live plugin instance"));

        syncing_ref.set(true);

        for row in &visible_rows {
            let pid_cb = pid_str.clone();
            let row_idx = row.index;
            let pm_cb = pm_render.clone();
            let sync_guard = syncing_ref.clone();

            match row.row_type.as_str() {
                "EntryRow" => {
                    let er = EntryRow::new();
                    er.set_title(&glib::markup_escape_text(&row.title));
                    er.set_text(&row.text);
                    er.set_editable(row.editable);
                    er.set_sensitive(row.sensitive);

                    er.connect_changed(move |e| {
                        if sync_guard.get() {
                            return;
                        }
                        let txt = e.text().to_string();
                        pm_cb.borrow().bridge.set_plugin_settings_row(
                            &pid_cb,
                            row_idx,
                            None,
                            None,
                            Some(txt),
                            None,
                            None,
                        );
                    });

                    group.add(&er);
                    rendered_widgets_ref.borrow_mut().push(er.upcast());
                }
                "PasswordRow" => {
                    let pr = PasswordEntryRow::new();
                    pr.set_title(&glib::markup_escape_text(&row.title));
                    pr.set_text(&row.text);
                    pr.set_editable(row.editable);
                    pr.set_sensitive(row.sensitive);

                    pr.connect_changed(move |e| {
                        if sync_guard.get() {
                            return;
                        }
                        let txt = e.text().to_string();
                        pm_cb.borrow().bridge.set_plugin_settings_row(
                            &pid_cb,
                            row_idx,
                            None,
                            None,
                            Some(txt),
                            None,
                            None,
                        );
                    });

                    group.add(&pr);
                    rendered_widgets_ref.borrow_mut().push(pr.upcast());
                }
                "SwitchRow" => {
                    let sr = SwitchRow::new();
                    sr.set_title(&glib::markup_escape_text(&row.title));
                    if !row.subtitle.is_empty() {
                        sr.set_subtitle(&glib::markup_escape_text(&row.subtitle));
                    }
                    sr.set_active(row.active);
                    sr.set_sensitive(row.sensitive);

                    sr.connect_active_notify(move |s| {
                        if sync_guard.get() {
                            return;
                        }
                        pm_cb.borrow().bridge.set_plugin_settings_row(
                            &pid_cb,
                            row_idx,
                            None,
                            Some(s.is_active()),
                            None,
                            None,
                            None,
                        );
                    });

                    group.add(&sr);
                    rendered_widgets_ref.borrow_mut().push(sr.upcast());
                }
                "ComboRow" => {
                    let str_opts: Vec<&str> = row.options.iter().map(|s| s.as_str()).collect();
                    let model = gtk::StringList::new(&str_opts);
                    let cr = ComboRow::new();
                    cr.set_title(&glib::markup_escape_text(&row.title));
                    if !row.subtitle.is_empty() {
                        cr.set_subtitle(&glib::markup_escape_text(&row.subtitle));
                    }
                    cr.set_model(Some(&model));
                    if row.selected < row.options.len() {
                        cr.set_selected(row.selected as u32);
                    }
                    cr.set_sensitive(row.sensitive);

                    cr.connect_selected_notify(move |c| {
                        if sync_guard.get() {
                            return;
                        }
                        pm_cb.borrow().bridge.set_plugin_settings_row(
                            &pid_cb,
                            row_idx,
                            Some(c.selected() as usize),
                            None,
                            None,
                            None,
                            None,
                        );
                    });

                    group.add(&cr);
                    rendered_widgets_ref.borrow_mut().push(cr.upcast());
                }
                "SpinRow" => {
                    let step = if row.step > 0.0 { row.step } else { 1.0 };
                    let max_v = if row.max > row.min { row.max } else { row.min + 100.0 };
                    let adj = gtk::Adjustment::new(row.value, row.min, max_v, step, step * 5.0, 0.0);
                    let sp = SpinRow::new(Some(&adj), step, row.digits);
                    sp.set_title(&glib::markup_escape_text(&row.title));
                    if !row.subtitle.is_empty() {
                        sp.set_subtitle(&glib::markup_escape_text(&row.subtitle));
                    }
                    sp.set_sensitive(row.sensitive);

                    sp.connect_value_notify(move |s| {
                        if sync_guard.get() {
                            return;
                        }
                        pm_cb.borrow().bridge.set_plugin_settings_row(
                            &pid_cb,
                            row_idx,
                            None,
                            None,
                            None,
                            None,
                            Some(s.value()),
                        );
                    });

                    group.add(&sp);
                    rendered_widgets_ref.borrow_mut().push(sp.upcast());
                }
                "ScaleRow" => {
                    let ar = ActionRow::new();
                    ar.set_title(&glib::markup_escape_text(&row.title));
                    if !row.subtitle.is_empty() {
                        ar.set_subtitle(&glib::markup_escape_text(&row.subtitle));
                    }
                    let step = if row.step > 0.0 { row.step } else { 1.0 };
                    let max_v = if row.max > row.min { row.max } else { row.min + 100.0 };
                    let scale = Scale::with_range(Orientation::Horizontal, row.min, max_v, step);
                    scale.set_value(row.value);
                    scale.set_digits(row.digits as i32);
                    scale.set_draw_value(true);
                    scale.set_size_request(200, -1);
                    scale.set_valign(gtk::Align::Center);
                    scale.set_sensitive(row.sensitive);

                    scale.connect_value_changed(move |s| {
                        if sync_guard.get() {
                            return;
                        }
                        pm_cb.borrow().bridge.set_plugin_settings_row(
                            &pid_cb,
                            row_idx,
                            None,
                            None,
                            None,
                            None,
                            Some(s.value()),
                        );
                    });

                    ar.add_suffix(&scale);
                    group.add(&ar);
                    rendered_widgets_ref.borrow_mut().push(scale.upcast());
                }
                "ColorRow" => {
                    let ar = ActionRow::new();
                    ar.set_title(&glib::markup_escape_text(&row.title));
                    if !row.subtitle.is_empty() {
                        ar.set_subtitle(&glib::markup_escape_text(&row.subtitle));
                    }
                    let dlg = ColorDialog::builder().with_alpha(false).build();
                    let cb = ColorDialogButton::new(Some(dlg));
                    cb.set_rgba(&hex_to_rgba(&row.color));
                    cb.set_valign(gtk::Align::Center);
                    cb.set_sensitive(row.sensitive);

                    cb.connect_rgba_notify(move |c| {
                        if sync_guard.get() {
                            return;
                        }
                        let hex = rgba_to_hex(&c.rgba());
                        pm_cb.borrow().bridge.set_plugin_settings_row(
                            &pid_cb,
                            row_idx,
                            None,
                            None,
                            None,
                            Some(hex),
                            None,
                        );
                    });

                    ar.add_suffix(&cb);
                    group.add(&ar);
                    rendered_widgets_ref.borrow_mut().push(cb.upcast());
                }
                "ButtonRow" => {
                    let ar = ActionRow::new();
                    ar.set_title(&glib::markup_escape_text(&row.title));
                    if !row.subtitle.is_empty() {
                        ar.set_subtitle(&glib::markup_escape_text(&row.subtitle));
                    }

                    let btn = if !row.button_label.is_empty() {
                        Button::with_label(&row.button_label)
                    } else if !row.button_icon.is_empty() {
                        Button::from_icon_name(&row.button_icon)
                    } else {
                        Button::with_label("Run")
                    };
                    btn.set_valign(gtk::Align::Center);
                    btn.set_sensitive(row.sensitive);
                    if row.suggested {
                        btn.add_css_class("suggested-action");
                    }

                    btn.connect_clicked(move |_| {
                        pm_cb.borrow().bridge.trigger_plugin_settings_button(&pid_cb, row_idx);
                    });

                    ar.add_suffix(&btn);
                    group.add(&ar);
                    rendered_widgets_ref.borrow_mut().push(btn.upcast());
                }
                _ => {
                    let ar = ActionRow::new();
                    ar.set_title(&glib::markup_escape_text(&row.title));
                    if !row.subtitle.is_empty() {
                        ar.set_subtitle(&glib::markup_escape_text(&row.subtitle));
                    }
                    group.add(&ar);
                    rendered_widgets_ref.borrow_mut().push(ar.upcast());
                }
            }
        }

        holder_ref.append(&group);
        *rendered_rows_ref.borrow_mut() = visible_rows;
        syncing_ref.set(false);
    });

    // Register live callback so incoming BridgeEvent::PluginSettingsRows updates this dialog live!
    let render_for_registry = render_rows.clone();
    *active_dialog_cb.borrow_mut() = Some((
        plugin_id.to_string(),
        std::boxed::Box::new(move |rows: &[BridgeConfigRow]| {
            render_for_registry(rows);
        }),
    ));

    // Clear registry entry on window close
    let reg_close = active_dialog_cb.clone();
    window.connect_close_request(move |_| {
        *reg_close.borrow_mut() = None;
        glib::Propagation::Proceed
    });

    // Immediately render cached rows if available, and request fresh live rows from the bridge
    if let Some(cached) = plugin_manager.borrow().get_live_plugin_settings_rows(plugin_id) {
        render_rows(&cached);
    }
    plugin_manager.borrow().bridge.get_plugin_settings_rows(plugin_id);

    window.present();
}
