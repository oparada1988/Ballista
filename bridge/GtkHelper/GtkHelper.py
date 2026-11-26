import os
import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, Gdk, GLib, Gio
from loguru import logger


def better_disconnect(widget, handler):
    try:
        widget.disconnect_by_func(handler)
    except Exception:
        pass


def better_unparent(widget):
    try:
        if widget and widget.get_parent() is not None:
            widget.unparent()
    except Exception:
        pass


def get_focused_widgets(start: Gtk.Widget) -> list:
    widgets = []
    while start is not None:
        child = start.get_focus_child()
        if child is None:
            break
        widgets.append(child)
        start = child
    return widgets


def get_deepest_focused_widget(start: Gtk.Widget):
    w = get_focused_widgets(start)
    return w[-1] if w else start


def get_deepest_focused_widget_with_attr(start: Gtk.Widget, attr: str):
    for w in reversed(get_focused_widgets(start)):
        if hasattr(w, attr):
            return w
    return None


def let_user_select_folder(callback, initial_folder: str = None, parent=None):
    dialog = Gtk.FileDialog.new()
    dialog.set_modal(True)
    if initial_folder and os.path.isdir(initial_folder):
        dialog.set_initial_folder(Gio.File.new_for_path(initial_folder))

    def on_selected(d, task):
        try:
            folder = d.select_folder_finish(task)
            if folder and callback:
                callback(folder.get_path())
        except Exception as e:
            logger.debug(f"Folder selection cancelled: {e}")

    dialog.select_folder(parent, None, on_selected)


class BetterExpander(Adw.ExpanderRow):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._rows = []
        self._on_rows_changed_callbacks = []

    def add_row(self, widget):
        super().add_row(widget)
        if widget not in self._rows:
            self._rows.append(widget)
        self._notify_rows_changed()

    def remove(self, widget):
        try:
            super().remove(widget)
        except Exception:
            pass
        if widget in self._rows:
            self._rows.remove(widget)
        self._notify_rows_changed()

    def get_rows(self):
        return list(self._rows)

    def clear(self):
        for r in list(self._rows):
            try:
                super().remove(r)
            except Exception:
                pass
        self._rows.clear()
        self._notify_rows_changed()

    def _notify_rows_changed(self):
        for cb in list(self._on_rows_changed_callbacks):
            try:
                cb()
            except Exception:
                pass


class BetterPreferencesGroup(Adw.PreferencesGroup):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._rows = []

    def add(self, widget):
        super().add(widget)
        self._rows.append(widget)

    def get_rows(self):
        return list(self._rows)

    def clear(self):
        for r in list(self._rows):
            try:
                self.remove(r)
            except Exception:
                pass
        self._rows.clear()


class ComboRow(Adw.ActionRow):
    """Legacy StreamController GtkHelper.GtkHelper.ComboRow backed by Gtk.ComboBox + Gtk.ListStore."""
    def __init__(self, title: str = "", model=None, *args, **kwargs):
        super().__init__(title=title, *args, **kwargs)
        self.combo_box = Gtk.ComboBox(valign=Gtk.Align.CENTER)
        if model is not None:
            self.combo_box.set_model(model)
        self.cell_renderer = Gtk.CellRendererText()
        self.combo_box.pack_start(self.cell_renderer, True)
        self.combo_box.add_attribute(self.cell_renderer, "text", 0)
        self.add_suffix(self.combo_box)


class ScaleRow(Adw.ActionRow):
    """Legacy StreamController GtkHelper.GtkHelper.ScaleRow."""
    def __init__(
        self,
        title: str = "",
        value: float = 0.0,
        min: float = 0.0,
        max: float = 100.0,
        step: float = 1.0,
        text_right: str = None,
        text_left: str = None,
        *args,
        **kwargs,
    ):
        super().__init__(title=title, *args, **kwargs)
        self.scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, min, max, step)
        self.scale.set_value(value)
        self.scale.set_hexpand(True)
        self.scale.set_size_request(180, -1)
        self.scale.set_valign(Gtk.Align.CENTER)
        self.add_suffix(self.scale)


class EntryRowWithoutTitle(Adw.EntryRow):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)


class AttributeRow(Adw.ActionRow):
    def __init__(self, title: str = "", attr: str = ""):
        super().__init__(title=title, subtitle=str(attr))


class EntryDialog(Adw.Window):
    def __init__(self, parent_window=None, dialog_title: str = "", entry_heading: str = "", default_text: str = "", confirm_label: str = "Confirm", **kwargs):
        super().__init__(title=dialog_title, transient_for=parent_window, modal=True)
        self._default_text = default_text

    def show(self, callback_func=None):
        if callback_func:
            callback_func(self._default_text)


class BackButton(Gtk.Button):
    def __init__(self, *args, **kwargs):
        super().__init__(icon_name="go-previous-symbolic", *args, **kwargs)


class RevertButton(Gtk.Button):
    def __init__(self, *args, **kwargs):
        super().__init__(icon_name="edit-undo-symbolic", *args, **kwargs)


class LoadingScreen(Gtk.Spinner):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    def set_spinning(self, loading: bool):
        super().set_spinning(loading)


class ErrorPage(Gtk.Box):
    def __init__(self, reload_func=None, error_text: str = "", reload_args=None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL)
        self.status_page = Adw.StatusPage(title="Error", description=error_text)
        self.append(self.status_page)
        self.reload_func = reload_func
        self.reload_args = reload_args
