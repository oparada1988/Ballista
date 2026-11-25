import re
import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw
from GtkHelper.GtkHelper import better_disconnect


class ScaleRow(Adw.ActionRow):
    def __init__(
        self,
        value: float = 0.0,
        min: float = 0.0,
        max: float = 100.0,
        draw_side_values: bool = True,
        add_text_entry: bool = False,
        title: str = None,
        subtitle: str = None,
        step: float = 0.1,
        digits: int = 2,
        draw_value: bool = False,
        round_digits: bool = True,
        text_entry_max_length: int = 6,
    ):
        super().__init__(title=title or "", subtitle=subtitle or "")
        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, hexpand=True)
        self._add_text_entry = add_text_entry
        self._draw_side_values = draw_side_values

        self.left = Gtk.Label(label=str(min), hexpand=False, halign=Gtk.Align.END)
        self.right = Gtk.Label(label=str(max), hexpand=False, halign=Gtk.Align.START)
        self.left.set_visible(self._draw_side_values)
        self.right.set_visible(self._draw_side_values)

        self._adjustment = Gtk.Adjustment.new(value, min, max, step, 1, 0)
        self.scale = Gtk.Scale(orientation=Gtk.Orientation.HORIZONTAL, adjustment=self._adjustment, hexpand=True)
        self.scale.set_draw_value(draw_value)
        self.scale.set_round_digits(round_digits)
        self.scale.set_digits(digits)

        if self._add_text_entry:
            self.entry_row = Gtk.Entry(valign=Gtk.Align.CENTER)
            self.entry_row.set_text(str(value))
            self.entry_row.set_max_length(text_entry_max_length)
            self.entry_row_controller = Gtk.EventControllerFocus()
            self.entry_row.add_controller(self.entry_row_controller)

        box.append(self.left)
        box.append(self.scale)
        box.append(self.right)
        self._connect_signals()
        self.add_suffix(box)
        if self._add_text_entry:
            self.add_suffix(self.entry_row)

    def _connect_signals(self):
        self._adjustment.connect("value-changed", self._correct_step_amount)
        if self._add_text_entry:
            self.entry_row.connect("activate", self._reset_entry_row)
            self.entry_row.connect("changed", self._entry_row_changed)

    def _disconnect_signals(self):
        better_disconnect(self._adjustment, self._correct_step_amount)
        if self._add_text_entry:
            better_disconnect(self.entry_row, self._entry_row_changed)
            better_disconnect(self.entry_row, self._reset_entry_row)

    def get_value(self) -> float:
        return self._adjustment.get_value()

    def set_value(self, value: float):
        self._adjustment.set_value(value)

    @property
    def min(self):
        return self._adjustment.get_lower()

    @min.setter
    def min(self, value: float):
        if self._adjustment.get_upper() >= value:
            self._adjustment.set_lower(value)
            self.left.set_label(str(value))

    @property
    def max(self):
        return self._adjustment.get_upper()

    @max.setter
    def max(self, value: float):
        if value >= self._adjustment.get_lower():
            self._adjustment.set_upper(value)
            self.right.set_label(str(value))

    @property
    def step(self):
        return self._adjustment.get_step_increment()

    @step.setter
    def step(self, value: float):
        self._adjustment.set_step_increment(value)

    @property
    def digits(self):
        return self.scale.get_digits()

    @digits.setter
    def digits(self, digits: int):
        self.scale.set_digits(digits)

    def _correct_step_amount(self, adjustment):
        value = adjustment.get_value()
        step = adjustment.get_step_increment()
        if step > 0:
            rounded = round(value / step) * step
            if abs(rounded - value) > 1e-9:
                adjustment.set_value(rounded)
                value = rounded
        if self._add_text_entry:
            self._disconnect_signals()
            self.entry_row.set_text(str(value))
            self._connect_signals()

    def _entry_row_changed(self, entry_row):
        self._disconnect_signals()
        text = re.sub(r"[^0-9.-]", "", entry_row.get_text())
        try:
            val = float(text)
            val = min(max(val, self._adjustment.get_lower()), self._adjustment.get_upper())
            self.scale.set_value(val)
        except ValueError:
            pass
        self._connect_signals()

    def _reset_entry_row(self, *args):
        self._disconnect_signals()
        self.entry_row.set_text(str(self._adjustment.get_value()))
        self._connect_signals()
