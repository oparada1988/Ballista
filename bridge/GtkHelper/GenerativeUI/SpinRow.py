import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw
from GtkHelper.GenerativeUI.GenerativeUI import GenerativeUI
from GtkHelper.GtkHelper import better_disconnect


class SpinRow(GenerativeUI[float]):
    def __init__(
        self,
        action_core,
        var_name: str,
        default_value: float = 0.0,
        min: float = 0.0,
        max: float = 100.0,
        title: str = None,
        subtitle: str = None,
        step: float = 1.0,
        digits: int = 0,
        on_change=None,
        can_reset: bool = True,
        auto_add: bool = True,
        complex_var_name: bool = False,
    ):
        super().__init__(action_core, var_name, default_value, can_reset, auto_add, complex_var_name, on_change)
        adj = Gtk.Adjustment.new(float(default_value), float(min), float(max), float(step), float(step) * 5, 0)
        self._widget = Adw.SpinRow(
            title=self.get_translation(title, title),
            subtitle=self.get_translation(subtitle, subtitle),
            adjustment=adj,
            digits=digits,
        )
        self.connect_signals()

    def connect_signals(self):
        self._widget.connect("notify::value", self._value_changed)

    def disconnect_signals(self):
        better_disconnect(self._widget, self._value_changed)

    def _value_changed(self, spin, _):
        val = spin.get_value()
        if spin.get_digits() == 0:
            val = int(round(val))
        self._handle_value_changed(val)

    @GenerativeUI.signal_manager
    def set_ui_value(self, value: float):
        try:
            self._widget.set_value(float(value))
        except Exception:
            pass

    def set_number(self, number: float, update_setting: bool = False):
        self.set_ui_value(number)
        if update_setting:
            self.set_value(number)

    def get_number(self) -> float:
        return self._widget.get_value()

    @property
    def min(self):
        return self._widget.get_adjustment().get_lower()

    @min.setter
    def min(self, value: float):
        self._widget.get_adjustment().set_lower(float(value))

    @property
    def max(self):
        return self._widget.get_adjustment().get_upper()

    @max.setter
    def max(self, value: float):
        self._widget.get_adjustment().set_upper(float(value))
