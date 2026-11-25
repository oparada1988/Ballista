use crate::core::PluginManager;
use gtk::prelude::*;
use gtk::{
    Box, Button, Image, Label, ListBox, Orientation, Revealer, RevealerTransitionType,
    ScrolledWindow, SearchEntry,
};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SelectedAction {
    pub plugin_id: String,
    pub plugin_name: String,
    pub action_id: String,
    pub action_name: String,
    pub icon_name: String,
}

pub struct Sidebar {
    pub container: Box,
    pub search_entry: SearchEntry,
    pub list_box: ListBox,
    pub filter_items: Rc<RefCell<Vec<(Box, Revealer, Image, String)>>>,
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
        }
    }

    pub fn populate_from_plugins<F>(&self, plugin_manager: &PluginManager, on_action_selected: F)
    where
        F: Fn(SelectedAction) + 'static + Clone,
    {
        // Clear existing rows
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }
        self.filter_items.borrow_mut().clear();

        // 1. Built-in Core System Actions (Collapsible)
        let core_actions = vec![
            ("system.hotkey", "Hotkey Trigger", "input-keyboard-symbolic"),
            ("system.url", "Open Website", "web-browser-symbolic"),
            ("system.terminal", "Run Terminal Command", "utilities-terminal-symbolic"),
        ];

        self.append_collapsible_category(
            "System",
            "system",
            &core_actions,
            on_action_selected.clone(),
        );

        // 2. Dynamically Discovered Real Plugins (e.g. AppLauncher)
        if plugin_manager.plugins.is_empty() {
            let empty_hint = Box::new(Orientation::Vertical, 8);
            empty_hint.set_margin_top(16);
            empty_hint.set_margin_start(16);
            empty_hint.set_margin_end(16);

            let hint_label = Label::new(Some(
                "No external plugins installed.\nClick the Store (:::+) icon above to install App Launcher!",
            ));
            hint_label.set_wrap(true);
            hint_label.add_css_class("dim-label");
            hint_label.set_xalign(0.0);
            empty_hint.append(&hint_label);

            self.list_box.append(&empty_hint);
        } else {
            for (plugin_id, plugin) in &plugin_manager.plugins {
                let actions_tuples: Vec<(&str, &str, &str)> = plugin
                    .actions
                    .iter()
                    .map(|a| (a.id.as_str(), a.name.as_str(), a.icon_name_or_file.as_str()))
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
        actions: &[(&str, &str, &str)],
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

        for (action_id, action_name, icon_name) in actions {
            let row = Box::new(Orientation::Horizontal, 12);
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
            let title = Label::new(Some(action_name));
            title.set_xalign(0.0);
            title.add_css_class("heading");

            let sub = Label::new(Some(category_name));
            sub.set_xalign(0.0);
            sub.add_css_class("dim-label");

            label_box.append(&title);
            label_box.append(&sub);
            row.append(&icon);
            row.append(&label_box);
            row.set_cursor_from_name(Some("grab"));

            let sel_action = SelectedAction {
                plugin_id: plugin_id.to_string(),
                plugin_name: category_name.to_string(),
                action_id: action_id.to_string(),
                action_name: action_name.to_string(),
                icon_name: icon_name.to_string(),
            };

            // 1. Drag and Drop: Allow dragging action item onto any Key, Dial, or Touch slot
            let drag_source = gtk::DragSource::new();
            drag_source.set_actions(gtk::gdk::DragAction::COPY);
            let drag_data = sel_action.clone();
            drag_source.connect_prepare(move |_src, _x, _y| {
                let json = serde_json::to_string(&drag_data).unwrap_or_default();
                Some(gtk::gdk::ContentProvider::for_value(&json.to_value()))
            });

            // Set the dragged icon to the action icon (clean icon, zero text clutter)
            let icon_for_drag = icon_name.to_string();
            let get_drag_paintable = move || -> Option<gtk::gdk::Paintable> {
                if std::path::Path::new(&icon_for_drag).exists() {
                    crate::ui::canvas::lookup_icon_paintable(Some(&icon_for_drag), None, 36, 6.0)
                } else {
                    crate::ui::canvas::lookup_icon_paintable(None, Some(&icon_for_drag), 36, 6.0)
                }
            };

            if let Some(paintable) = get_drag_paintable() {
                drag_source.set_icon(Some(&paintable), 18, 18);
            }

            let icon_drag_begin = icon_name.to_string();
            drag_source.connect_drag_begin(move |src, _drag| {
                let p = if std::path::Path::new(&icon_drag_begin).exists() {
                    crate::ui::canvas::lookup_icon_paintable(Some(&icon_drag_begin), None, 36, 6.0)
                } else {
                    crate::ui::canvas::lookup_icon_paintable(None, Some(&icon_drag_begin), 36, 6.0)
                };
                if let Some(paintable) = p {
                    src.set_icon(Some(&paintable), 18, 18);
                }
            });

            row.add_controller(drag_source);

            // 2. Right-Click Context Menu: "Add to Key"
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

            let add_item_btn = Button::with_label("Add to Key");
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
