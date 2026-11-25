import os
import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, Gio


class FileDialogFilter:
    def __init__(self, name: str = "", patterns: list = None, mime_types: list = None):
        self.name = name
        self.patterns = patterns or []
        self.mime_types = mime_types or []


class FileDialogRow(Adw.ActionRow):
    def __init__(
        self,
        title: str = None,
        subtitle: str = None,
        dialog_title: str = None,
        initial_path: str = None,
        block_interaction: bool = True,
        only_show_filename: bool = False,
        filters=None,
        file_change_callback=None,
    ):
        super().__init__(title=title or "", subtitle=subtitle or "")
        self.dialog_title = dialog_title or title or "Select File"
        self.only_show_filename = only_show_filename
        self.filters = filters or []
        self.file_change_callback = file_change_callback
        self.selected_file_path = initial_path or ""
        self.button = Gtk.Button(label=self._format_label(), valign=Gtk.Align.CENTER)
        self.button.connect("clicked", self.on_open_dialog_clicked)
        self.add_suffix(self.button)

    def _format_label(self) -> str:
        if not self.selected_file_path:
            return "Select File..."
        return os.path.basename(self.selected_file_path) if self.only_show_filename else self.selected_file_path

    def load_from_path(self, path: str):
        self.selected_file_path = path or ""
        self.update_label()

    def update_label(self):
        self.button.set_label(self._format_label())

    def on_open_dialog_clicked(self, button):
        dialog = Gtk.FileDialog.new()
        dialog.set_title(self.dialog_title)
        dialog.open(None, None, self.on_file_dialog_response)

    def on_file_dialog_response(self, dialog, task):
        try:
            f = dialog.open_finish(task)
            if f:
                self.selected_file_path = f.get_path() or ""
                self.update_label()
                if self.file_change_callback:
                    self.file_change_callback(self.selected_file_path)
        except Exception:
            pass
