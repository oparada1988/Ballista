import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw


class ConfigGroup(Adw.PreferencesGroup):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)


class CommentGroup(Adw.PreferencesGroup):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
