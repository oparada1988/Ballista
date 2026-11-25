import enum
import json
from types import MappingProxyType
from .Observer import Observer
from .Asset import Asset


class ManagerEvent(enum.Enum):
    ADD = "add"
    REMOVE = "remove"
    OVERRIDE_ADD = "override_add"
    OVERRIDE_REMOVE = "override_remove"


class Manager:
    def __init__(self, asset_type: type, json_key: str):
        self._asset_type = asset_type
        self._assets: dict = {}
        self._asset_overrides: dict = {}
        self._observer = Observer()
        self._json_key = json_key

    def add_asset(self, key: str, asset: Asset, override: bool = False):
        if key not in self._assets or override:
            self._assets[key] = asset
            self._observer.notify(ManagerEvent.ADD, key, asset)

    def remove_asset(self, key: str):
        if key in self._assets:
            del self._assets[key]
            self._observer.notify(ManagerEvent.REMOVE, key, None)
        if key in self._asset_overrides:
            del self._asset_overrides[key]

    def add_override(self, key: str, asset: Asset, skip_asset_check: bool = False, override: bool = False):
        if key not in self._assets and not skip_asset_check:
            return
        if key not in self._asset_overrides or override:
            self._asset_overrides[key] = asset
            self._observer.notify(ManagerEvent.OVERRIDE_ADD, key, asset)

    def remove_override(self, key: str):
        if key in self._asset_overrides:
            del self._asset_overrides[key]
            self._observer.notify(ManagerEvent.OVERRIDE_REMOVE, key, self.get_asset(key))

    def get_asset(self, key: str, skip_override: bool = False):
        if skip_override:
            return self._assets.get(key, None)
        return self._asset_overrides.get(key, self._assets.get(key, None))

    def get_asset_values(self, key: str, skip_override: bool = False):
        asset = self.get_asset(key, skip_override)
        return asset.get_values() if asset else None

    def get_assets(self):
        return MappingProxyType(self._assets)

    def get_overrides(self):
        return MappingProxyType(self._asset_overrides)

    def get_assets_merged(self):
        return MappingProxyType({**self._assets, **self._asset_overrides})

    def add_listener(self, callback):
        self._observer.subscribe(callback)

    def remove_listener(self, callback):
        self._observer.unsubscribe(callback)

    def get_asset_json(self):
        return json.dumps({k: v.to_json() for k, v in self._assets.items()}, indent=4)

    def get_override_json(self):
        return {k: v.to_json() for k, v in self._asset_overrides.items()}

    def load_json(self, json_data: dict):
        section = json_data.get(self._json_key)
        if not section or not isinstance(section, dict):
            return
        for key, value in section.items():
            try:
                self.add_override(key, self._asset_type.from_json(value), skip_asset_check=True)
            except Exception:
                pass

    def get_save_key(self) -> str:
        return self._json_key
