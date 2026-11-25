from GtkHelper.ToggleRow import ToggleRow as ToggleWidget
from GtkHelper.GenerativeUI.GenerativeUI import GenerativeUI
from GtkHelper.GtkHelper import better_disconnect


class ToggleRow(GenerativeUI[int]):
    def __init__(
        self,
        action_core,
        var_name: str,
        default_value=0,
        toggles=None,
        title: str = None,
        subtitle: str = None,
        can_shrink: bool = False,
        homogeneous: bool = False,
        active: int = 0,
        on_change=None,
        can_reset: bool = True,
        auto_add: bool = True,
        complex_var_name: bool = False,
    ):
        super().__init__(action_core, var_name, default_value, can_reset, auto_add, complex_var_name, on_change)
        self._widget = ToggleWidget(
            toggles=toggles,
            title=self.get_translation(title, title),
            subtitle=self.get_translation(subtitle, subtitle),
            can_shrink=can_shrink,
            homogeneous=homogeneous,
            active=active,
        )
        self.connect_signals()

    def connect_signals(self):
        self._widget.connect("notify::selected", self._value_changed)

    def disconnect_signals(self):
        better_disconnect(self._widget, self._value_changed)

    def _value_changed(self, combo, _):
        self._handle_value_changed(combo.get_active_index())

    @GenerativeUI.signal_manager
    def set_ui_value(self, value):
        if isinstance(value, int):
            self._widget.set_selected(value)
        else:
            for idx, t in enumerate(self._widget.get_toggles()):
                if getattr(t, "name", str(t)) == str(value):
                    self._widget.set_selected(idx)
                    break

    def get_toggles(self):
        return self._widget.get_toggles()

    def get_n_toggles(self) -> int:
        return self._widget.get_n_toggles()

    def get_toggle_by_name(self, name: str):
        return self._widget.get_toggle_by_name(name)
