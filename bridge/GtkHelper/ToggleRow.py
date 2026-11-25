import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw


class ToggleRow(Adw.ComboRow):
    """Compatibility ToggleRow exposing selectable toggle items as a ComboRow."""
    def __init__(
        self,
        toggles=None,
        active_toggle=None,
        title: str = None,
        subtitle: str = None,
        can_shrink: bool = False,
        homogeneous: bool = False,
        active: int = 0,
    ):
        self._toggles = list(toggles or [])
        labels = [getattr(t, "label", None) or getattr(t, "name", None) or str(t) for t in self._toggles]
        super().__init__(title=title or "", subtitle=subtitle or "", model=Gtk.StringList.new(labels))
        if active_toggle is not None:
            for idx, t in enumerate(self._toggles):
                if t == active_toggle or getattr(t, "name", None) == active_toggle:
                    active = idx
                    break
        if labels:
            self.set_selected(min(max(0, active), len(labels) - 1))

    def get_toggles(self):
        return list(self._toggles)

    def get_n_toggles(self) -> int:
        return len(self._toggles)

    def get_toggle_by_name(self, name: str):
        for t in self._toggles:
            if getattr(t, "name", None) == name or str(t) == name:
                return t
        return None

    def get_toggle_at(self, index: int):
        return self._toggles[index] if 0 <= index < len(self._toggles) else None

    def get_active_index(self) -> int:
        sel = self.get_selected()
        return sel if sel != 4294967295 else 0

    def get_active_toggle(self):
        return self.get_toggle_at(self.get_active_index())

    def get_active_name(self) -> str:
        t = self.get_active_toggle()
        return getattr(t, "name", str(t)) if t is not None else ""
