import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, Gdk


class ColorButtonRow(Adw.ActionRow):
    def __init__(self, title: str = None, subtitle: str = None, default_color: tuple = (0, 0, 0, 255)):
        super().__init__(title=title or "", subtitle=subtitle or "")
        self.color_button = Gtk.ColorButton(valign=Gtk.Align.CENTER)
        self.add_suffix(self.color_button)
        self.color = default_color

    @property
    def color(self) -> tuple:
        rgba = self.color_button.get_rgba()
        return self.convert_from_rgba(rgba)

    @color.setter
    def color(self, value: tuple):
        rgba = self.convert_to_rgba(value)
        self.color_button.set_rgba(rgba)
        self.color_button.emit("color-set")

    def convert_from_rgba(self, color: Gdk.RGBA) -> tuple:
        return self.normalize_to_255((color.red, color.green, color.blue, color.alpha))

    def convert_to_rgba(self, color: tuple) -> Gdk.RGBA:
        vals = list(color)
        while len(vals) < 4:
            vals.append(255)
        norm = self.normalize_to_1(tuple(vals[:4]))
        rgba = Gdk.RGBA()
        rgba.red, rgba.green, rgba.blue, rgba.alpha = norm
        return rgba

    def normalize_to_255(self, color: tuple) -> tuple:
        return tuple(round(v * 255) for v in color)

    def normalize_to_1(self, color: tuple) -> tuple:
        return tuple(v / 255.0 for v in color)
