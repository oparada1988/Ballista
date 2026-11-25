import gi
gi.require_version("Adw", "1")
from gi.repository import Adw
from GtkHelper.GenerativeUI.GenerativeUI import GenerativeUI
from GtkHelper.GtkHelper import better_disconnect


class SwitchRow(GenerativeUI[bool]):
    def __init__(
        self,
        action_core,
        var_name: str,
        default_value: bool = False,
        title: str = None,
        subtitle: str = None,
        on_change=None,
        can_reset: bool = True,
        auto_add: bool = True,
        complex_var_name: bool = False,
    ):
        super().__init__(action_core, var_name, default_value, can_reset, auto_add, complex_var_name, on_change)
        self._widget = Adw.SwitchRow(
            title=self.get_translation(title, title),
            subtitle=self.get_translation(subtitle, subtitle),
        )
        self._widget.set_active(bool(default_value))
        self.connect_signals()

    def connect_signals(self):
        self._widget.connect("notify::active", self._value_changed)

    def disconnect_signals(self):
        better_disconnect(self._widget, self._value_changed)

    def _value_changed(self, switch, _):
        self._handle_value_changed(switch.get_active())

    @GenerativeUI.signal_manager
    def set_ui_value(self, value: bool):
        self._widget.set_active(bool(value))

    def set_active(self, active: bool, change_setting: bool = False):
        self.set_ui_value(active)
        if change_setting:
            self.set_value(bool(active))

    def get_active(self) -> bool:
        return self._widget.get_active()
