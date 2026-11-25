import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, Gio, GObject
from loguru import logger


class BaseComboRowItem(GObject.GObject):
    def __init__(self):
        super().__init__()

    def __str__(self):
        return ""

    def get_value(self):
        return self.__str__()

    def __eq__(self, other):
        if isinstance(other, BaseComboRowItem):
            return self.get_value() == other.get_value()
        return self.get_value() == str(other)

    @GObject.Property(type=GObject.TYPE_STRING)
    def filter_value(self):
        return self.__str__()


class ComboRowItem(BaseComboRowItem):
    def __init__(self, label: str):
        super().__init__()
        self.label = str(label)

    def __str__(self):
        return self.label


class SimpleComboRowItem(BaseComboRowItem):
    def __init__(self, value, label: str):
        super().__init__()
        self.value = value
        self.label = str(label)

    def __str__(self):
        return self.label

    def get_value(self):
        return self.value


class ComboRow(Adw.ComboRow):
    def __init__(
        self,
        items=None,
        title: str = None,
        subtitle: str = None,
        enable_search: bool = True,
        default_selection=None,
    ):
        super().__init__(title=title or "", subtitle=subtitle or "")
        self.model = Gio.ListStore(item_type=GObject.GObject)
        factory = Gtk.SignalListItemFactory()
        factory.connect("setup", self._on_factory_setup)
        factory.connect("bind", self._on_factory_bind)
        self.set_model(self.model)
        self.set_factory(factory)
        self.set_enable_search(enable_search)
        self.set_expression(Gtk.PropertyExpression.new(BaseComboRowItem, None, "filter_value"))
        if items:
            self.populate(self.convert_item_list(items), default_selection)

    def convert_item_list(self, items) -> list:
        converted = []
        for item in (items or []):
            if isinstance(item, BaseComboRowItem):
                converted.append(item)
            elif isinstance(item, str):
                converted.append(ComboRowItem(item))
            else:
                converted.append(ComboRowItem(str(item)))
        return converted

    def set_selected_item(self, item):
        selected_idx = 0
        n = self.model.get_n_items()
        for idx in range(n):
            if self.model.get_item(idx) == item:
                selected_idx = idx
                break
        if n > 0:
            self.set_selected(selected_idx)
            return self.get_item_at(selected_idx)
        return None

    def add_item(self, combo_row_item):
        if isinstance(combo_row_item, str):
            combo_row_item = ComboRowItem(combo_row_item)
        self.model.append(combo_row_item)

    def add_items(self, items):
        converted = self.convert_item_list(items)
        if converted:
            self.model.splice(self.model.get_n_items(), 0, converted)

    def remove_item_at_index(self, index: int):
        if 0 <= index < self.model.get_n_items():
            self.model.remove(index)

    def remove_item(self, item):
        for idx in range(self.model.get_n_items()):
            if self.model.get_item(idx) == item:
                self.remove_item_at_index(idx)
                break

    def remove_items(self, start: int, amount: int):
        size = self.model.get_n_items()
        if 0 <= start < size:
            for _ in range(min(amount + 1, size - start)):
                self.model.remove(start)

    def remove_all_items(self):
        self.model.remove_all()

    def get_item_at(self, index: int):
        if 0 <= index < self.model.get_n_items():
            return self.model.get_item(index)
        return None

    def get_item(self, name):
        for idx in range(self.model.get_n_items()):
            it = self.model.get_item(idx)
            if it and it == name:
                return it
        return None

    def get_item_amount(self) -> int:
        return self.model.get_n_items()

    def get_selected_item(self):
        sel = self.get_selected()
        if sel == 4294967295 or sel < 0:
            return None
        return self.get_item_at(sel)

    def populate(self, items, selected_item=""):
        self.remove_all_items()
        self.add_items(items)
        if selected_item not in (None, ""):
            self.set_selected_item(selected_item)
        elif self.model.get_n_items() > 0:
            self.set_selected(0)

    def _on_factory_setup(self, factory, list_item):
        list_item.set_child(Gtk.Label(halign=Gtk.Align.START))

    def _on_factory_bind(self, factory, list_item):
        item = list_item.get_item()
        lbl = list_item.get_child()
        if item and lbl:
            lbl.set_text(str(item))
