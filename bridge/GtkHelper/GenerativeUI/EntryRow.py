import gi
gi.require_version("Adw", "1")
from gi.repository import Adw
from GtkHelper.GenerativeUI.GenerativeUI import GenerativeUI
from GtkHelper.GtkHelper import better_disconnect


class EntryRow(GenerativeUI[str]):
    def __init__(
        self,
        action_core,
        var_name: str,
        default_value: str = "",
        title: str = None,
        filter_func=None,
        on_change=None,
        can_reset: bool = True,
        auto_add: bool = True,
        complex_var_name: bool = False,
    ):
        super().__init__(action_core, var_name, default_value, can_reset, auto_add, complex_var_name, on_change)
        self.filter_func = filter_func
        self._widget = Adw.EntryRow(title=self.get_translation(title, title))
        self._widget.set_text(str(default_value or ""))
        self.connect_signals()

    def connect_signals(self):
        self._widget.connect("changed", self._value_changed)

    def disconnect_signals(self):
        better_disconnect(self._widget, self._value_changed)

    def _value_changed(self, entry):
        text = entry.get_text()
        if self.filter_func and not self.filter_func(text):
            return
        self._handle_value_changed(text)

    @GenerativeUI.signal_manager
    def set_ui_value(self, value: str):
        self._widget.set_text(str(value or ""))

    def set_text(self, text: str, update_setting: bool = False):
        self.set_ui_value(text)
        if update_setting:
            self.set_value(text)

    def get_text(self) -> str:
        return self._widget.get_text()
