from GtkHelper.ScaleRow import ScaleRow as ScaleWidget
from GtkHelper.GenerativeUI.GenerativeUI import GenerativeUI
from GtkHelper.GtkHelper import better_disconnect


class ScaleRow(GenerativeUI[float]):
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
        digits: int = 2,
        draw_value: bool = False,
        round_digits: bool = True,
        add_text_entry: bool = False,
        text_entry_max_length: int = 6,
        on_change=None,
        can_reset: bool = True,
        auto_add: bool = True,
        complex_var_name: bool = False,
    ):
        super().__init__(action_core, var_name, default_value, can_reset, auto_add, complex_var_name, on_change)
        self._widget = ScaleWidget(
            value=float(default_value),
            min=float(min),
            max=float(max),
            title=self.get_translation(title, title),
            subtitle=self.get_translation(subtitle, subtitle),
            step=float(step),
            digits=digits,
            draw_value=draw_value,
            round_digits=round_digits,
            add_text_entry=add_text_entry,
            text_entry_max_length=text_entry_max_length,
        )
        self.connect_signals()

    def connect_signals(self):
        self._widget.scale.connect("value-changed", self._value_changed)

    def disconnect_signals(self):
        better_disconnect(self._widget.scale, self._value_changed)

    def _value_changed(self, scale):
        self._handle_value_changed(scale.get_value())

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

    def set_min(self, min_val: float):
        self._widget.min = float(min_val)

    def set_max(self, max_val: float):
        self._widget.max = float(max_val)
