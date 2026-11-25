import os
from PIL import Image


class Media:
    def __init__(self, image=None, path: str = None, size: float = 1.0, valign: float = 0.0, halign: float = 0.0):
        self.image = image
        self.path = path
        self.size = size
        self.valign = valign
        self.halign = halign

    @classmethod
    def from_path(cls, path: str, size: float = 1.0, valign: float = 0.0, halign: float = 0.0):
        img = None
        if path and os.path.isfile(path) and not path.lower().endswith(".svg"):
            try:
                with Image.open(path) as opened:
                    img = opened.copy()
            except Exception:
                img = None
        return cls(image=img, path=path, size=size, valign=valign, halign=halign)

    def get_final_media(self):
        return self.image
