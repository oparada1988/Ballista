import gi
gi.require_version("Adw", "1")
from gi.repository import Adw
from GtkHelper.GenerativeUI.GenerativeUI import GenerativeUI
from GtkHelper.GtkHelper import better_disconnect


class PasswordEntryRow(GenerativeUI[str]):
    def __init__(
        self,
        action_core,
        var_name: str,
        default_value: str = "",
        title: str = None,
        on_change=None,
        can_reset: bool = True,
        auto_add: bool = True,
        complex_var_name: bool = False,
    ):
        super().__init__(action_core, var_name, default_value, can_reset, auto_add, complex_var_name, on_change)
        self._widget = Adw.PasswordEntryRow(title=self.get_translation(title, title))
        self._widget.set_text(str(default_value or ""))
        self.connect_signals()

    def connect_signals(self):
        self._widget.connect("changed", self._value_changed)

    def disconnect_signals(self):
        better_disconnect(self._widget, self._value_changed)

    def _value_changed(self, entry):
        self._handle_value_changed(entry.get_text())

    @GenerativeUI.signal_manager
    def set_ui_value(self, value: str):
        self._widget.set_text(str(value or ""))

    def set_password(self, password: str, update_setting: bool = False):
        self.set_ui_value(password)
        if update_setting:
            self.set_value(password)

    def get_password(self) -> str:
        return self._widget.get_text()
