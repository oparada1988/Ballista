from GtkHelper.ComboRow import ComboRow as Combo, BaseComboRowItem
from GtkHelper.GenerativeUI.GenerativeUI import GenerativeUI
from GtkHelper.GtkHelper import better_disconnect


class ComboRow(GenerativeUI[BaseComboRowItem]):
    def __init__(
        self,
        action_core,
        var_name: str,
        default_value,
        items,
        title: str = None,
        subtitle: str = None,
        enable_search: bool = False,
        on_change=None,
        can_reset: bool = True,
        auto_add: bool = True,
        complex_var_name: bool = False,
    ):
        super().__init__(action_core, var_name, default_value, can_reset, auto_add, complex_var_name, on_change)
        self._widget = Combo(
            title=self.get_translation(title, title),
            subtitle=self.get_translation(subtitle, subtitle),
            items=items,
            enable_search=enable_search,
            default_selection=self._default_value,
        )
        self.connect_signals()

    def connect_signals(self):
        self._widget.connect("notify::selected", self._value_changed)

    def disconnect_signals(self):
        better_disconnect(self._widget, self._value_changed)

    def _value_changed(self, combo_row, _):
        item = combo_row.get_selected_item()
        self._handle_value_changed(item)

    def _handle_value_changed(self, item, update_settings: bool = True, trigger_callback: bool = True):
        old_value = self.get_value(self._default_value)
        if update_settings:
            self.set_value(item)
        if trigger_callback and self.on_change:
            old_item = self.get_item(old_value)
            self.on_change(self._widget, item, old_item)

    @GenerativeUI.signal_manager
    def reset_value(self):
        selected_item = self._widget.set_selected_item(self._default_value)
        self._handle_value_changed(selected_item)

    def load_initial_ui(self):
        value = self.get_value()
        selected_item = self._widget.set_selected_item(value)
        self._handle_value_changed(selected_item, False)

    @GenerativeUI.signal_manager
    def set_ui_value(self, value):
        self._widget.set_selected_item(value)

    def set_value(self, item):
        val = item.get_value() if isinstance(item, BaseComboRowItem) else item
        super().set_value(val)

    def set_selected_item(self, item="", update_setting: bool = False):
        sel = self._widget.set_selected_item(item)
        if update_setting:
            self.set_value(sel)
        return sel

    @GenerativeUI.signal_manager
    def add_item(self, item):
        self._widget.add_item(item)

    @GenerativeUI.signal_manager
    def add_items(self, items):
        self._widget.add_items(items)

    @GenerativeUI.signal_manager
    def remove_item_at_index(self, index: int):
        self._widget.remove_item_at_index(index)

    @GenerativeUI.signal_manager
    def remove_item(self, item):
        self._widget.remove_item(item)

    @GenerativeUI.signal_manager
    def remove_items(self, start: int, amount: int):
        self._widget.remove_items(start, amount)

    @GenerativeUI.signal_manager
    def remove_all_items(self):
        self._widget.remove_all_items()

    def get_item_at(self, index: int):
        return self._widget.get_item_at(index)

    def get_item(self, name):
        return self._widget.get_item(name)

    def get_selected_item(self):
        return self._widget.get_selected_item()

    def get_item_amount(self) -> int:
        return self._widget.get_item_amount()

    @GenerativeUI.signal_manager
    def populate(self, items, selected_item="", update_settings: bool = False, trigger_callback: bool = True):
        self._widget.remove_all_items()
        self._widget.add_items(items)
        sel = self._widget.set_selected_item(selected_item)
        self._handle_value_changed(sel, update_settings, trigger_callback)
        self._widget.set_selected_item(sel)
