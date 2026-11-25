import gi
gi.require_version("Adw", "1")
from gi.repository import Adw
from GtkHelper.GenerativeUI.GenerativeUI import GenerativeUI
from GtkHelper.GtkHelper import better_disconnect


class ExpanderRow(GenerativeUI[bool]):
    def __init__(
        self,
        action_core,
        var_name: str,
        default_value: bool = False,
        title: str = None,
        subtitle: str = None,
        show_enable_switch: bool = False,
        start_expanded: bool = False,
        on_change=None,
        can_reset: bool = True,
        auto_add: bool = True,
        complex_var_name: bool = False,
    ):
        super().__init__(action_core, var_name, bool(default_value), can_reset, auto_add, complex_var_name, on_change)
        self._widget = Adw.ExpanderRow(
            title=self.get_translation(title, title),
            subtitle=self.get_translation(subtitle, subtitle),
            show_enable_switch=show_enable_switch,
            enable_expansion=bool(default_value) if show_enable_switch else True,
            expanded=start_expanded,
        )
        self._rows: list = []
        self.connect_signals()

    def connect_signals(self):
        self._widget.connect("notify::enable-expansion", self._value_changed)

    def disconnect_signals(self):
        better_disconnect(self._widget, self._value_changed)

    def _value_changed(self, row, _):
        self._handle_value_changed(row.get_enable_expansion())

    @GenerativeUI.signal_manager
    def set_ui_value(self, value: bool):
        self._widget.set_enable_expansion(bool(value))

    def set_enable_expansion(self, enable_expansion: bool, update_setting: bool = False):
        self.set_ui_value(enable_expansion)
        if update_setting:
            self.set_value(bool(enable_expansion))

    def get_enable_expansion(self) -> bool:
        return self._widget.get_enable_expansion()

    def add_row(self, widget):
        w = widget.widget if hasattr(widget, "widget") else widget
        if w is not None:
            self._widget.add_row(w)
            self._rows.append(w)

    def clear_rows(self):
        for r in list(self._rows):
            try:
                self._widget.remove(r)
            except Exception:
                pass
        self._rows.clear()
