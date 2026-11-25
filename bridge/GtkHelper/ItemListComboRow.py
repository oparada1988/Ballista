import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, Gio, GObject


class ItemListComboRowListItem(GObject.GObject):
    def __init__(self, key: str, name: str):
        super().__init__()
        self._key = key
        self._name = name

    @GObject.Property(type=str)
    def key(self):
        return self._key

    @GObject.Property(type=str)
    def name(self):
        return self._name

    def __str__(self):
        return self._name


class ItemListComboRow(Adw.ComboRow):
    def __init__(self, items=None, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._store = Gio.ListStore(item_type=ItemListComboRowListItem)
        self.set_model(self._store)
        self.set_expression(Gtk.PropertyExpression.new(ItemListComboRowListItem, None, "name"))
        if items:
            self.set_items(items)

    def set_items(self, items):
        self._store.remove_all()
        for it in (items or []):
            self._store.append(it)

    def set_selected_item_by_key(self, key: str, default: int = 0):
        for idx in range(self._store.get_n_items()):
            if self._store.get_item(idx).key == key:
                self.set_selected(idx)
                return
        if self._store.get_n_items() > 0:
            self.set_selected(default)
