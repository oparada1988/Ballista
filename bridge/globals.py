import os
import sys
from collections import deque
from loguru import logger

IS_MAC = sys.platform == "darwin"

top_level_dir: str = os.path.dirname(os.path.abspath(__file__))
DATA_PATH: str = os.path.expanduser("~/.local/share/ballista")
PLUGIN_DIR: str = os.path.join(DATA_PATH, "plugins")
os.makedirs(PLUGIN_DIR, exist_ok=True)

app_version: str = "1.5.0-beta.16"
threads_running: bool = True
screen_locked: bool = False
showed_donate_window: bool = False
fallback_font: str = "Cantarell"

video_extensions = ["mp4", "mov", "MP4", "MOV", "mkv", "MKV", "webm", "WEBM", "gif", "GIF"]
image_extensions = ["png", "jpg", "jpeg", "webp", "PNG", "JPG", "JPEG", "WEBP"]
svg_extensions = ["svg", "SVG"]

app_loading_finished_tasks: list = []
api_page_requests: dict = {}
api_state_requests: dict = {}
api_action_requests: dict = {}
logs = deque(maxlen=5000)


class _LoggerRegistry(dict):
    def get(self, key, default=None):
        return super().get(key, logger)


loggers = _LoggerRegistry({"plugins": logger, "global": logger})


class _SignalManagerProxy:
    def __init__(self):
        self._listeners: dict = {}

    def connect_signal(self, signal=None, callback=None):
        if signal is None or callback is None:
            return
        self._listeners.setdefault(signal, [])
        if callback not in self._listeners[signal]:
            self._listeners[signal].append(callback)

    def disconnect_signal(self, signal=None, callback=None):
        if signal in self._listeners and callback in self._listeners[signal]:
            self._listeners[signal].remove(callback)

    def trigger_signal(self, signal=None, *args, **kwargs):
        for cb in list(self._listeners.get(signal, [])):
            try:
                cb(*args, **kwargs)
            except Exception as e:
                logger.warning(f"Signal callback error for {signal}: {e}")


class _AppProxy:
    def __init__(self):
        self.main_win = None
        self._runner = None

    def send_notification(self, icon: str = "", title: str = "", body: str = "", button=None):
        logger.info(f"[Plugin Notification] {title}: {body}")
        if self._runner is not None:
            self._runner.emit("notification", {
                "icon": str(icon or ""),
                "title": str(title or ""),
                "body": str(body or ""),
            })

    def get_active_window(self):
        return self.main_win

    def get_windows(self):
        return [self.main_win] if self.main_win else []


class _PluginManagerProxy:
    def __init__(self):
        self.backends: list = []
        self.action_index: dict = {}
        self._runner = None

    def get_plugins(self, include_disabled: bool = False) -> dict:
        from src.backend.PluginManager.PluginBase import PluginBase
        res = dict(PluginBase.plugins)
        if include_disabled:
            res.update(PluginBase.disabled_plugins)
        return res

    def get_plugin_by_id(self, plugin_id: str, include_disabled: bool = True):
        entry = self.get_plugins(include_disabled).get(plugin_id)
        if isinstance(entry, dict):
            return entry.get("object")
        if self._runner is not None:
            return self._runner.plugins.get(plugin_id)
        return None

    def get_action_holder_from_id(self, action_id: str):
        return self.action_index.get(action_id)


class _DeckManagerProxy:
    def __init__(self):
        self.deck_controllers: list = []

    def get_deck_controller(self, serial_number: str = None):
        if not self.deck_controllers:
            return None
        if serial_number is None:
            return self.deck_controllers[0]
        for dc in self.deck_controllers:
            if getattr(dc, "serial_number", None) == serial_number:
                return dc
        return self.deck_controllers[0]


class _PageManagerProxy:
    def __init__(self):
        self.pages: list = []
        self._registered_pages: list = []

    def register_page(self, path: str):
        if path not in self._registered_pages:
            self._registered_pages.append(path)

    def unregister_page(self, path: str):
        if path in self._registered_pages:
            self._registered_pages.remove(path)

    def get_pages(self) -> list:
        return list(self.pages)

    def get_page(self, path: str, deck_controller=None):
        return deck_controller.active_page if deck_controller else None


class _FlatpakPermissionManagerProxy:
    def show_dbus_permission_request_dialog(self, name: str, bus: str = "session", description: str = None):
        logger.debug(f"Native Ballista environment: DBus permission '{name}' ({bus}) granted automatically")


class _MediaManagerProxy:
    def generate_svg_thumbnail(self, media_path: str):
        return None


class _AssetManagerProxy:
    def __init__(self):
        self.icons = {}
        self.colors = {}


class _SettingsManagerProxy:
    def get_app_settings(self) -> dict:
        return {}

    def get_deck_settings(self, serial: str = None) -> dict:
        return {}


class _FallbackLocaleManager:
    def get(self, key: str, fallback: str = None) -> str:
        return fallback if fallback is not None else key


signal_manager = _SignalManagerProxy()
app = _AppProxy()
plugin_manager = _PluginManagerProxy()
deck_manager = _DeckManagerProxy()
page_manager = _PageManagerProxy()
flatpak_permission_manager = _FlatpakPermissionManagerProxy()
media_manager = _MediaManagerProxy()
asset_manager = _AssetManagerProxy()
asset_manager_backend = _AssetManagerProxy()
settings_manager = _SettingsManagerProxy()
lm = _FallbackLocaleManager()
icon_pack_manager = None
wallpaper_pack_manager = None
sd_plus_bar_wallpaper_pack_manager = None
store_backend = None
pyro_daemon = None
window_grabber = None
lock_screen_detector = None
store = None
tray_icon = None
ai_manager = None
action_doc_registry = None
gnome_extensions = None
page_manager_window = None
