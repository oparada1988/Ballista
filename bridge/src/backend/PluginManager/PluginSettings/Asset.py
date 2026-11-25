from os.path import isfile
from src.backend.DeckManagement.Media.Media import Media


class Asset:
    def __init__(self, *args, **kwargs):
        pass

    def get_values(self):
        return None

    def to_json(self):
        return None

    @classmethod
    def from_json(cls, *args, **kwargs):
        return None


class Color(Asset):
    def __init__(self, color: tuple = (0, 0, 0, 255), *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._color = tuple(color)

    def get_values(self):
        return self._color

    def to_json(self):
        return list(self._color)

    @classmethod
    def from_json(cls, *args, **kwargs):
        return cls(color=tuple(args[0]))


class Icon(Asset):
    def __init__(self, path: str = "", size: float = 1.0, valign: float = 0.0, halign: float = 0.0, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.path = path
        if path and isfile(path):
            self._path = path
            self._icon = Media.from_path(path, size=size, valign=valign, halign=halign)
            self._rendered = self._icon.get_final_media()
        else:
            self._path = None
            self._icon = None
            self._rendered = None

    def get_values(self):
        return self._icon, self._rendered

    def to_json(self):
        return {
            "path": self._path,
            "size": self._icon.size if self._icon else 1.0,
            "halign": self._icon.halign if self._icon else 0.0,
            "valign": self._icon.valign if self._icon else 0.0,
        }

    @classmethod
    def from_json(cls, *args, **kwargs):
        data = args[0] if args else {}
        return cls(
            path=data.get("path", ""),
            size=data.get("size", 1.0),
            halign=data.get("halign", 0.0),
            valign=data.get("valign", 0.0),
        )
