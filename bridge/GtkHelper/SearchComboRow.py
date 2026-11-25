import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, Gio, GObject


class SearchComboRowItem(GObject.GObject):
    def __init__(self, display_label: str):
        super().__init__()
        self._display_label = str(display_label)

    @GObject.Property(type=str)
    def display_label(self):
        return self._display_label

    def __str__(self):
        return self._display_label


class SearchComboRow(Adw.ComboRow):
    def __init__(self, title: str = "", use_single_line: bool = True, *args, **kwargs):
        super().__init__(title=title, *args, **kwargs)
        self.model = Gio.ListStore(item_type=SearchComboRowItem)
        self.set_model(self.model)
        self.set_enable_search(True)
        self.set_expression(Gtk.PropertyExpression.new(SearchComboRowItem, None, "display_label"))

    def populate(self, items: list, selected_index: int = 0):
        self.model.remove_all()
        for it in (items or []):
            self.model.append(it if isinstance(it, SearchComboRowItem) else SearchComboRowItem(str(it)))
        if self.model.get_n_items() > 0:
            self.set_selected(min(selected_index, self.model.get_n_items() - 1))

    def set_selected_item(self, index: int):
        if 0 <= index < self.model.get_n_items():
            self.set_selected(index)
