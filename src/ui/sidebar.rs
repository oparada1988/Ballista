use crate::core::PluginManager;
use crate::native_sdk::TargetCaps;
use gtk::prelude::*;
use gtk::{
    Box, Button, Image, Label, ListBox, Orientation, Revealer, RevealerTransitionType,
    ScrolledWindow, SearchEntry,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SelectedAction {
    pub plugin_id: String,
    pub plugin_name: String,
    pub action_id: String,
    pub action_name: String,
    pub icon_name: String,
    #[serde(default)]
    pub caps: TargetCaps,
}

pub struct Sidebar {
    pub container: Box,
    pub search_entry: SearchEntry,
    pub list_box: ListBox,
    pub filter_items: Rc<RefCell<Vec<(Box, Revealer, Image, String)>>>,
    pub active_drag_caps: Rc<Cell<Option<TargetCaps>>>,
}

impl Sidebar {
    pub fn new() -> Self {
        let container = Box::new(Orientation::Vertical, 6);
        container.add_css_class("action-sidebar");
        container.set_size_request(240, -1);

        // Search Bar at Top
        let search_box = Box::new(Orientation::Horizontal, 4);
        search_box.set_margin_top(8);
        search_box.set_margin_bottom(6);
        search_box.set_margin_start(10);
        search_box.set_margin_end(10);

        let search_entry = SearchEntry::new();
        search_entry.set_placeholder_text(Some("Search actions..."));
        search_entry.set_hexpand(true);
        search_box.append(&search_entry);
        container.append(&search_box);

        // Actions Scrolled Window
        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .build();

        let list_box = ListBox::new();
        list_box.set_selection_mode(gtk::SelectionMode::None);

        scrolled.set_child(Some(&list_box));
        container.append(&scrolled);

        let filter_items: Rc<RefCell<Vec<(Box, Revealer, Image, String)>>> =
            Rc::new(RefCell::new(Vec::new()));

        // Wire search filtering
        let items_search = filter_items.clone();
        search_entry.connect_search_changed(move |entry| {
            let q = entry.text().trim().to_lowercase();
            let items = items_search.borrow();
            for (row, revealer, arrow, search_text) in items.iter() {
                if q.is_empty() {
                    row.set_visible(true);
                } else if search_text.contains(&q) {
                    row.set_visible(true);
                    revealer.set_reveal_child(true);
                    arrow.set_icon_name(Some("pan-down-symbolic"));
                } else {
                    row.set_visible(false);
                }
            }
        });

        Self {
            container,
            search_entry,
            list_box,
            filter_items,
            active_drag_caps: Rc::new(Cell::new(None)),
        }
    }

    pub fn set_drag_caps_cell(&mut self, cell: Rc<Cell<Option<TargetCaps>>>) {
        self.active_drag_caps = cell;
    }

    pub fn populate_from_plugins<F>(
        &self,
        plugin_manager: &PluginManager,
        native_host: &crate::native_sdk::NativePluginHost,
        on_action_selected: F,
    ) where
        F: Fn(SelectedAction) + 'static + Clone,
    {
        // Clear existing rows
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }
        self.filter_items.borrow_mut().clear();

        // 1. Native Rust Ballista Built-In Plugins ("Ballista" core suite + any future native plugins)
        for (plugin_id, plugin_name, descriptors) in native_host.list_plugins() {
            let visible_actions: Vec<(&str, &str, &str, Option<TargetCaps>)> = descriptors
                .iter()
                .filter(|d| d.show_in_sidebar)
                .map(|d| (d.id.as_str(), d.name.as_str(), d.icon_name.as_str(), Some(d.caps)))
                .collect();

            if !visible_actions.is_empty() {
                self.append_collapsible_category(
                    &plugin_name,
                    &plugin_id,
                    &visible_actions,
                    on_action_selected.clone(),
                );
            }
        }

        // 2. Dynamically Discovered StreamController / External Plugins
        if plugin_manager.plugins.is_empty() {
            let empty_hint = Box::new(Orientation::Vertical, 8);
            empty_hint.set_margin_top(16);
            empty_hint.set_margin_start(16);
            empty_hint.set_margin_end(16);

            let hint_label = Label::new(Some(
                "No external plugins installed.\nClick the Store icon above to install plugins!",
            ));
            hint_label.set_wrap(true);
            hint_label.add_css_class("dim-label");
            hint_label.set_xalign(0.0);
            empty_hint.append(&hint_label);

            self.list_box.append(&empty_hint);
        } else {
            for (plugin_id, plugin) in &plugin_manager.plugins {
                let actions_tuples: Vec<(&str, &str, &str, Option<TargetCaps>)> = plugin
                    .actions
                    .iter()
                    .map(|a| (a.id.as_str(), a.name.as_str(), a.icon_name_or_file.as_str(), a.caps))
                    .collect();

                self.append_collapsible_category(
                    &plugin.name,
                    plugin_id,
                    &actions_tuples,
                    on_action_selected.clone(),
                );
            }
        }
    }

    fn append_collapsible_category<F>(
        &self,
        category_name: &str,
        plugin_id: &str,
        actions: &[(&str, &str, &str, Option<TargetCaps>)],
        on_action_selected: F,
    ) where
        F: Fn(SelectedAction) + 'static + Clone,
    {
        let group_box = Box::new(Orientation::Vertical, 2);
        group_box.add_css_class("sidebar-category-group");

        // Clickable Collapsible Header Button with Arrow
        let header_btn = Button::new();
        header_btn.add_css_class("sidebar-category-btn");
        header_btn.add_css_class("flat");

        let header_content = Box::new(Orientation::Horizontal, 8);
        header_content.set_margin_start(10);
        header_content.set_margin_end(10);
        header_content.set_margin_top(7);
        header_content.set_margin_bottom(7);

        let arrow_icon = Image::from_icon_name("pan-down-symbolic");
        arrow_icon.set_pixel_size(12);
        arrow_icon.add_css_class("sidebar-category-arrow");

        let title_label = Label::new(Some(category_name));
        title_label.set_xalign(0.0);
        title_label.set_hexpand(true);
        title_label.add_css_class("sidebar-category-title");

        let count_badge = Label::new(Some(&format!("{}", actions.len())));
        count_badge.add_css_class("sidebar-category-badge");

        header_content.append(&arrow_icon);
        header_content.append(&title_label);
        header_content.append(&count_badge);
        header_btn.set_child(Some(&header_content));

        // Revealer for smooth expand / collapse animation
        let revealer = Revealer::new();
        revealer.set_transition_type(RevealerTransitionType::SlideDown);
        revealer.set_reveal_child(true);

        let actions_box = Box::new(Orientation::Vertical, 2);
        actions_box.set_margin_start(4);
        actions_box.set_margin_end(4);
        actions_box.set_margin_bottom(4);

        for (action_id, action_name, icon_name, explicit_caps) in actions {
            let caps = explicit_caps.unwrap_or(TargetCaps::KEYS_AND_DIALS);
            let row = Box::new(Orientation::Horizontal, 10);
            row.add_css_class("action-row");

            let icon = if std::path::Path::new(icon_name).exists() {
                let img = Image::from_file(icon_name);
                img.set_pixel_size(18);
                img
            } else {
                let img = Image::from_icon_name(icon_name);
                img.set_pixel_size(18);
                img
            };

            let label_box = Box::new(Orientation::Vertical, 1);
            label_box.set_hexpand(true);
            let title = Label::new(Some(action_name));
            title.set_xalign(0.0);
            title.set_ellipsize(pango::EllipsizeMode::End);
            title.add_css_class("heading");

            let sub = Label::new(Some(category_name));
            sub.set_xalign(0.0);
            sub.add_css_class("dim-label");

            label_box.append(&title);
            label_box.append(&sub);
            row.append(&icon);
            row.append(&label_box);

            // Surface Exclusivity Badge for Native Ballista Actions (KEY / DIAL / KEY · DIAL)
            if let Some(native_caps) = explicit_caps {
                let badge = Label::new(Some(native_caps.badge_label()));
                badge.add_css_class("cap-badge");
                if native_caps.keys && native_caps.dials {
                    badge.add_css_class("cap-badge-both");
                    badge.set_tooltip_text(Some("Works on LCD Keys & Rotary Dials (adapts layout)"));
                } else if native_caps.dials {
                    badge.add_css_class("cap-badge-dial");
                    badge.set_tooltip_text(Some("Dial Exclusive — drag onto a Stream Deck + Dial or Touch Strip"));
                } else {
                    badge.add_css_class("cap-badge-key");
                    badge.set_tooltip_text(Some("Key Exclusive — drag onto an LCD Key"));
                }
                badge.set_valign(gtk::Align::Center);
                row.append(&badge);
            }

            row.set_cursor_from_name(Some("grab"));

            let sel_action = SelectedAction {
                plugin_id: plugin_id.to_string(),
                plugin_name: category_name.to_string(),
                action_id: action_id.to_string(),
                action_name: action_name.to_string(),
                icon_name: icon_name.to_string(),
                caps,
            };

            // 1. Drag and Drop: Allow dragging action item onto compatible Key, Dial, or Touch slots
            let drag_source = gtk::DragSource::new();
            drag_source.set_actions(gtk::gdk::DragAction::COPY);
            let drag_data = sel_action.clone();
            drag_source.connect_prepare(move |_src, _x, _y| {
                let json = serde_json::to_string(&drag_data).unwrap_or_default();
                Some(gtk::gdk::ContentProvider::for_value(&json.to_value()))
            });

            // Set the dragged cursor preview to a compact, uniform 44x44 rounded Midnight chip
            let icon_for_drag = icon_name.to_string();
            let get_drag_paintable = move || -> Option<gtk::gdk::Paintable> {
                let tex = if std::path::Path::new(&icon_for_drag).exists() {
                    crate::ui::canvas::create_drag_chip_texture(Some(&icon_for_drag), None)
                } else {
                    crate::ui::canvas::create_drag_chip_texture(None, Some(&icon_for_drag))
                };
                tex.map(|t| t.upcast())
            };

            if let Some(paintable) = get_drag_paintable() {
                drag_source.set_icon(Some(&paintable), 22, 22);
            }

            let icon_drag_begin = icon_name.to_string();
            let drag_caps_begin = self.active_drag_caps.clone();
            drag_source.connect_drag_begin(move |src, _drag| {
                drag_caps_begin.set(Some(caps));
                let tex = if std::path::Path::new(&icon_drag_begin).exists() {
                    crate::ui::canvas::create_drag_chip_texture(Some(&icon_drag_begin), None)
                } else {
                    crate::ui::canvas::create_drag_chip_texture(None, Some(&icon_drag_begin))
                };
                if let Some(t) = tex {
                    let paintable: gtk::gdk::Paintable = t.upcast();
                    src.set_icon(Some(&paintable), 22, 22);
                }
            });

            let drag_caps_end = self.active_drag_caps.clone();
            drag_source.connect_drag_end(move |_src, _drag, _del| {
                drag_caps_end.set(None);
            });

            let drag_caps_cancel = self.active_drag_caps.clone();
            drag_source.connect_drag_cancel(move |_src, _drag, _reason| {
                drag_caps_cancel.set(None);
                true
            });

            row.add_controller(drag_source);

            // 2. Right-Click Context Menu: target-aware Add label
            let right_click = gtk::GestureClick::new();
            right_click.set_button(3); // Secondary button (right-click)

            let popover = gtk::Popover::new();
            popover.set_has_arrow(true);
            popover.add_css_class("menu");

            let pop_box = Box::new(Orientation::Vertical, 2);
            pop_box.set_margin_top(4);
            pop_box.set_margin_bottom(4);
            pop_box.set_margin_start(4);
            pop_box.set_margin_end(4);

            let add_label = if caps.keys && caps.dials {
                "Add to Selected Slot"
            } else if caps.dials {
                "Add to Dial"
            } else {
                "Add to Key"
            };
            let add_item_btn = Button::with_label(add_label);
            add_item_btn.add_css_class("flat");
            let pop_weak = popover.downgrade();
            let cb = on_action_selected.clone();
            let sel_menu = sel_action.clone();
            add_item_btn.connect_clicked(move |_| {
                if let Some(p) = pop_weak.upgrade() {
                    p.popdown();
                }
                cb(sel_menu.clone());
            });
            pop_box.append(&add_item_btn);
            popover.set_child(Some(&pop_box));
            popover.set_parent(&row);

            let pop_show = popover.clone();
            right_click.connect_pressed(move |_g, _n, x, y| {
                let rect = gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
                pop_show.set_pointing_to(Some(&rect));
                pop_show.popup();
            });
            row.add_controller(right_click);

            actions_box.append(&row);

            self.filter_items.borrow_mut().push((
                row,
                revealer.clone(),
                arrow_icon.clone(),
                format!("{} {}", category_name.to_lowercase(), action_name.to_lowercase()),
            ));
        }

        revealer.set_child(Some(&actions_box));

        // Connect Header Click to toggle Revealer and Arrow
        let rev_toggle = revealer.clone();
        let arrow_toggle = arrow_icon.clone();
        header_btn.connect_clicked(move |_| {
            let will_reveal = !rev_toggle.reveals_child();
            rev_toggle.set_reveal_child(will_reveal);
            arrow_toggle.set_icon_name(Some(if will_reveal {
                "pan-down-symbolic"
            } else {
                "pan-end-symbolic"
            }));
        });

        group_box.append(&header_btn);
        group_box.append(&revealer);
        self.list_box.append(&group_box);
    }
}
