from GtkHelper.ColorButtonRow import ColorButtonRow as ColorWidget
from GtkHelper.GenerativeUI.GenerativeUI import GenerativeUI
from GtkHelper.GtkHelper import better_disconnect


class ColorButtonRow(GenerativeUI[tuple]):
    def __init__(
        self,
        action_core,
        var_name: str,
        default_value: tuple = (0, 0, 0, 255),
        title: str = None,
        subtitle: str = None,
        on_change=None,
        can_reset: bool = True,
        auto_add: bool = True,
        complex_var_name: bool = False,
    ):
        super().__init__(action_core, var_name, tuple(default_value), can_reset, auto_add, complex_var_name, on_change)
        self._widget = ColorWidget(
            title=self.get_translation(title, title),
            subtitle=self.get_translation(subtitle, subtitle),
            default_color=tuple(default_value),
        )
        self.connect_signals()

    def connect_signals(self):
        self._widget.color_button.connect("color-set", self._value_changed)

    def disconnect_signals(self):
        better_disconnect(self._widget.color_button, self._value_changed)

    def _value_changed(self, btn):
        self._handle_value_changed(self._widget.color)

    @GenerativeUI.signal_manager
    def set_ui_value(self, value):
        if isinstance(value, (list, tuple)):
            self._widget.color = tuple(value)

    def set_color(self, color, update_setting: bool = False):
        self.set_ui_value(color)
        if update_setting:
            self.set_value(list(color))

    def get_color(self) -> tuple:
        return self._widget.color

    def convert_from_rgba(self, color):
        return self._widget.convert_from_rgba(color)

    def convert_to_rgba(self, color):
        return self._widget.convert_to_rgba(color)
