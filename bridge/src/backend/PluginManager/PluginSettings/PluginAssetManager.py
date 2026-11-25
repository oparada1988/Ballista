import json
import os
from .Manager import Manager
from .Asset import Color, Icon
from src.backend.Utils.AtomicSaveUtils import atomic_save_json


class AssetManager:
    def __init__(self, plugin_base=None):
        self.plugin_base = plugin_base
        self.colors = Manager(Color, "colors")
        self.icons = Manager(Icon, "icons")

    def load_assets(self):
        if not self.plugin_base or not os.path.exists(self.plugin_base.settings_path):
            return {}
        try:
            with open(self.plugin_base.settings_path, "r", encoding="utf-8") as f:
                data = json.load(f)
                assets = data.get("assets", {}) if isinstance(data, dict) else {}
                self.icons.load_json(assets)
                self.colors.load_json(assets)
        except Exception:
            pass

    def save_assets(self):
        if not self.plugin_base:
            return
        content = {}
        if os.path.isfile(self.plugin_base.settings_path):
            try:
                with open(self.plugin_base.settings_path, "r", encoding="utf-8") as f:
                    content = json.load(f)
            except Exception:
                content = {}
        content["assets"] = {
            self.colors.get_save_key(): self.colors.get_override_json(),
            self.icons.get_save_key(): self.icons.get_override_json(),
        }
        atomic_save_json(self.plugin_base.settings_path, content, indent=4)
