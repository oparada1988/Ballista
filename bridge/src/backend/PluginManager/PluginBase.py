import configparser
import importlib
import inspect
import json
import os
import subprocess
import sys
import threading
import time
from functools import lru_cache
from loguru import logger

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, Gdk

import globals as gl
from locales.LegacyLocaleManager import LegacyLocaleManager
from locales.LocaleManager import LocaleManager
from src.backend.PluginManager.ActionHolder import ActionHolder
from src.backend.PluginManager.ActionHolderGroup import ActionHolderGroup
from src.backend.PluginManager.EventHolder import EventHolder
from src.backend.PluginManager.PluginSettings.Asset import Icon, Color
from src.backend.PluginManager.PluginSettings.PluginAssetManager import AssetManager
from src.backend.Utils.AtomicSaveUtils import atomic_save_json

try:
    import rpyc
    from rpyc.utils.server import ThreadedServer
    _RPYC_BASE = rpyc.Service
except ImportError:
    rpyc = None
    ThreadedServer = None
    _RPYC_BASE = object


class PluginBase(_RPYC_BASE):
    plugins = {}
    disabled_plugins = {}
    backend_spawn_count = 0

    BACKEND_CONNECT_TIMEOUT: float = 30.0
    PENDING_EVENT_TIMEOUT: float = 30.0

    def __init__(self, use_legacy_locale: bool = True, legacy_dir: str = "locales"):
        self.backend_connection = None
        self.backend = None
        self.server = None
        self.backend_process = None
        self.backend_launch_pending: bool = False
        self._pending_action_events: list = []
        self._pending_action_events_lock = threading.Lock()

        self.logger = gl.loggers.get("plugins", logger)
        self.PATH = os.path.dirname(inspect.getfile(self.__class__))
        self.plugin_id = self.get_plugin_id()

        self.settings_dir = os.path.join(gl.DATA_PATH, "settings", "plugins", self.plugin_id)
        self.fallback_settings_dir = os.path.expanduser(
            f"~/.var/app/com.core447.StreamController/data/settings/plugins/{self.plugin_id}"
        )
        self.settings_path = os.path.join(self.settings_dir, "settings.json")

        if use_legacy_locale:
            self.locale_manager = LegacyLocaleManager(os.path.join(self.PATH, legacy_dir))
        else:
            self.locale_manager = LocaleManager(os.path.join(self.PATH, "locales.csv"))
        self.locale_manager.set_to_os_default()

        self.action_holders: dict = {}
        self.action_holder_groups: set = set()
        self.event_holders: dict = {}
        self.registered: bool = False
        self.plugin_name: str = None
        self.github_repo: str = None
        self.plugin_version: str = None
        self.min_app_version: str = None
        self.app_version: str = None

        self.asset_manager = AssetManager(self)
        self.asset_manager.load_assets()

        self.has_plugin_settings: bool = False
        self.first_setup: bool = True
        self.registered_pages: list[str] = []

    @lru_cache(maxsize=1)
    def get_plugin_id(self) -> str:
        manifest = self.get_manifest()
        return manifest.get("id") or self.get_plugin_id_from_folder_name()

    def get_plugin_id_from_folder_name(self) -> str:
        return os.path.basename(os.path.abspath(self.PATH))

    def get_manifest(self) -> dict:
        manifest_path = os.path.join(self.PATH, "manifest.json")
        if os.path.exists(manifest_path):
            try:
                with open(manifest_path, "r", encoding="utf-8") as f:
                    return json.load(f)
            except Exception:
                pass
        return {}

    def get_about(self) -> dict:
        about_path = os.path.join(self.PATH, "about.json")
        if os.path.exists(about_path):
            try:
                with open(about_path, "r", encoding="utf-8") as f:
                    return json.load(f)
            except Exception:
                pass
        return {}

    def register(
        self,
        plugin_name: str = None,
        github_repo: str = None,
        plugin_version: str = None,
        app_version: str = None,
    ):
        manifest = self.get_manifest()
        self.plugin_name = plugin_name or manifest.get("name") or self.plugin_id
        self.github_repo = github_repo or manifest.get("github") or ""
        self.plugin_version = plugin_version or manifest.get("version") or "0.1.0"
        self.min_app_version = manifest.get("minimum-app-version")
        self.app_version = app_version or manifest.get("app-version") or gl.app_version
        self.plugin_id = self.get_plugin_id()

        PluginBase.plugins[self.plugin_id] = {
            "object": self,
            "plugin_version": self.plugin_version,
            "minimum_app_version": self.min_app_version,
            "github": self.github_repo,
            "folder_path": self.PATH,
            "file_name": os.path.basename(inspect.getfile(self.__class__)),
        }
        self.registered = True
        settings = self.get_settings()
        self.first_setup = settings.get("first-setup", True)

    def get_settings(self) -> dict:
        candidates = [
            self.settings_path,
            os.path.join(self.fallback_settings_dir, "settings.json"),
        ]
        # Also check folder-name settings path if manifest id differs from folder name
        folder_id = self.get_plugin_id_from_folder_name()
        if folder_id != self.plugin_id:
            candidates.append(os.path.join(gl.DATA_PATH, "settings", "plugins", folder_id, "settings.json"))
            candidates.append(
                os.path.expanduser(f"~/.var/app/com.core447.StreamController/data/settings/plugins/{folder_id}/settings.json")
            )

        for path in candidates:
            if os.path.isfile(path):
                try:
                    with open(path, "r", encoding="utf-8") as f:
                        data = json.load(f)
                        if isinstance(data, dict):
                            if data.get("file-version") == "2.0" or ("settings" in data and isinstance(data["settings"], dict)):
                                return dict(data.get("settings", {}))
                            return dict(data)
                except Exception as e:
                    logger.warning(f"Error loading settings from {path}: {e}")
        return {}

    def set_settings(self, settings: dict):
        try:
            content = {}
            if os.path.isfile(self.settings_path):
                try:
                    with open(self.settings_path, "r", encoding="utf-8") as f:
                        content = json.load(f)
                except Exception:
                    content = {}
            if isinstance(content, dict) and content.get("file-version") == "2.0":
                new_content = content.copy()
                new_content["settings"] = settings
            else:
                new_content = {
                    "file-version": "2.0",
                    "settings": settings,
                }
            atomic_save_json(self.settings_path, new_content, indent=4)
        except Exception as e:
            logger.error(f"Error writing settings to {self.settings_path}: {e}")

    def add_action_holder(self, action_holder: ActionHolder):
        self.action_holders[action_holder.action_id] = action_holder
        gl.plugin_manager.action_index[action_holder.action_id] = action_holder

    def add_action_holders(self, action_holders: list):
        for ah in action_holders:
            self.add_action_holder(ah)

    def add_action_holder_group(self, action_holder_group: ActionHolderGroup):
        self.action_holder_groups.add(action_holder_group)
        for ah in getattr(action_holder_group, "action_holders", []):
            self.add_action_holder(ah)

    def add_action_holder_groups(self, action_holder_groups: list):
        for grp in action_holder_groups:
            self.add_action_holder_group(grp)

    def get_action_holders(self) -> dict:
        return self.action_holders

    def add_event_holder(self, event_holder: EventHolder):
        self.event_holders[event_holder.event_id] = event_holder

    def add_event_holders(self, event_holders: list):
        for eh in event_holders:
            self.add_event_holder(eh)

    def connect_to_event(self, callback=None, event_id: str = None, event_id_suffix: str = None):
        full_id = event_id or (f"{self.get_plugin_id()}::{event_id_suffix}" if event_id_suffix else None)
        if full_id and full_id in self.event_holders and callback:
            self.event_holders[full_id].add_listener(callback)

    def connect_to_event_directly(self, plugin_id: str, event_id: str, callback):
        plugin = self.get_plugin(plugin_id)
        if plugin is not None:
            plugin.connect_to_event(callback=callback, event_id=event_id)

    def disconnect_from_event(self, event_id: str, callback):
        if event_id in self.event_holders:
            self.event_holders[event_id].remove_listener(callback)

    def disconnect_from_event_directly(self, plugin_id: str, event_id: str, callback):
        plugin = self.get_plugin(plugin_id)
        if plugin is not None:
            plugin.disconnect_from_event(event_id, callback)

    def get_plugin(self, plugin_id: str):
        return gl.plugin_manager.get_plugin_by_id(plugin_id)

    def add_icon(self, key: str, path: str, size: float = 1.0, halign: float = 0.0, valign: float = 0.0):
        self.asset_manager.icons.add_asset(key=key, asset=Icon(path=path, size=size, halign=halign, valign=valign))

    def add_color(self, key: str, color: tuple):
        self.asset_manager.colors.add_asset(key=key, asset=Color(color=color))

    def get_asset_path(self, asset_name: str, subdirs: list = None, asset_folder: str = "assets") -> str:
        if not subdirs:
            return os.path.join(self.PATH, asset_folder, asset_name)
        subdir = os.path.join(*subdirs)
        if subdir:
            return os.path.join(self.PATH, asset_folder, subdir, asset_name)
        return ""

    def add_css_stylesheet(self, path: str):
        try:
            display = Gdk.Display.get_default()
            if display and os.path.isfile(path):
                css_provider = Gtk.CssProvider()
                css_provider.load_from_path(path)
                Gtk.StyleContext.add_provider_for_display(
                    display, css_provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
                )
        except Exception as e:
            logger.debug(f"Could not load CSS stylesheet {path}: {e}")

    def register_page(self, path: str):
        gl.page_manager.register_page(path)
        self.registered_pages.append(path)

    def get_selector_icon(self) -> Gtk.Widget:
        return Gtk.Image(icon_name="view-paged")

    def get_settings_area(self):
        return None

    def get_config_rows(self) -> list:
        return []

    def request_dbus_permission(self, name: str, bus: str = "session", description: str = None):
        gl.flatpak_permission_manager.show_dbus_permission_request_dialog(name, bus, description)

    def save_tokens(self, *args, **kwargs):
        pass

    def on_close(self):
        pass

    def on_uninstall(self):
        for page in self.registered_pages:
            gl.page_manager.unregister_page(page)
        try:
            if self.backend is not None or self.backend_process is not None:
                self.on_disconnect(self.backend_connection)
        except Exception as e:
            logger.error(f"Error stopping plugin backend on uninstall: {e}")

    # ---------- RPyC Backend Support ---------- #
    def start_server(self):
        if rpyc is None or ThreadedServer is None:
            logger.warning(f"Plugin {self.plugin_id}: rpyc is not installed; skipping RPyC server startup")
            return
        if self.server is not None:
            return
        self.server = ThreadedServer(self, hostname="localhost", port=0, protocol_config={"allow_public_attrs": True})
        threading.Thread(target=self.server.start, name=f"rpyc_{self.plugin_id}", daemon=True).start()

    def on_disconnect(self, conn=None):
        if self.server is not None:
            try:
                self.server.close()
            except Exception:
                pass
        if self.backend_connection is not None:
            try:
                self.backend_connection.close()
            except Exception:
                pass
        if self.backend_process is not None and self.backend_process.poll() is None:
            logger.info(f"[backend] stopping plugin backend pid={self.backend_process.pid} plugin={self.plugin_id}")
            try:
                self.backend_process.terminate()
            except Exception:
                pass
        self.server = None
        self.backend_connection = None
        self.backend_process = None
        self.backend = None
        self.backend_launch_pending = False

    def is_backend_venv_healthy(self, venv_path: str) -> bool:
        pyvenv_path = os.path.join(venv_path, "pyvenv.cfg")
        if not os.path.exists(pyvenv_path):
            return False
        try:
            pyvenv = configparser.ConfigParser()
            with open(pyvenv_path, "r", encoding="utf-8") as f:
                pyvenv.read_string("[root]\n" + f.read())
            pyvenv_version = pyvenv["root"].get("version", "")
            system_version = f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}"
            return pyvenv_version == system_version
        except Exception:
            return False

    def recreate_venv(self, plugin_path: str):
        install_script = os.path.join(plugin_path, "__install__.py")
        if os.path.isfile(install_script):
            env = os.environ.copy()
            bridge_dir = gl.top_level_dir
            env["PYTHONPATH"] = f"{bridge_dir}:{env.get('PYTHONPATH', '')}".rstrip(":")
            subprocess.run([sys.executable, install_script], env=env, check=False)

    def launch_backend(self, backend_path: str, venv_path: str = None, open_in_terminal: bool = False):
        if self.backend_process is not None and self.backend_process.poll() is None:
            return
        if rpyc is None:
            logger.warning(f"Plugin {self.plugin_id} requested RPyC backend ({backend_path}), but python3 'rpyc' is not installed.")
            return
        if not os.path.exists(backend_path):
            logger.error(f"Backend path does not exist: {backend_path}")
            return

        if venv_path is not None and not self.is_backend_venv_healthy(venv_path):
            logger.info(f"Recreating venv for plugin {self.plugin_id}...")
            self.recreate_venv(self.PATH)

        self.start_server()
        if self.server is None:
            return
        port = self.server.port

        env = os.environ.copy()
        bridge_dir = gl.top_level_dir
        env["PYTHONPATH"] = f"{bridge_dir}:{env.get('PYTHONPATH', '')}".rstrip(":")

        if venv_path and os.path.isfile(os.path.join(venv_path, "bin", "python3")):
            python_bin = os.path.join(venv_path, "bin", "python3")
        else:
            python_bin = sys.executable

        cmd = [python_bin, backend_path, f"--port={port}"]
        logger.info(f"Launching plugin backend for {self.plugin_id}: {' '.join(cmd)}")
        self.backend_launch_pending = True
        self.backend_process = subprocess.Popen(cmd, env=env, start_new_session=open_in_terminal)
        PluginBase.backend_spawn_count += 1
        threading.Thread(target=self.wait_for_backend, name=f"wait_backend_{self.plugin_id}", daemon=True).start()

    def wait_for_backend(self, timeout: float = None):
        if timeout is None:
            timeout = self.BACKEND_CONNECT_TIMEOUT
        deadline = time.monotonic() + timeout
        while self.backend_connection is None and time.monotonic() < deadline:
            time.sleep(0.1)
        if self.backend_connection is None:
            logger.error(f"{self.plugin_id} - Could not connect to plugin backend within {timeout}s")
            self.backend_launch_pending = False
            self._flush_pending_action_events()

    def register_backend(self, port: int):
        if rpyc is None:
            return
        self.backend_connection = rpyc.connect("localhost", port, config={"allow_public_attrs": True})
        self.backend = self.backend_connection.root
        gl.plugin_manager.backends.append(self.backend_connection)
        self.backend_launch_pending = False
        self._flush_pending_action_events()

    def queue_action_event(self, retry) -> bool:
        if not self.backend_launch_pending:
            return False
        with self._pending_action_events_lock:
            if not self.backend_launch_pending:
                return False
            self._pending_action_events.append((time.monotonic(), retry))
        return True

    def _flush_pending_action_events(self):
        with self._pending_action_events_lock:
            pending = self._pending_action_events
            self._pending_action_events = []
        now = time.monotonic()
        for queued_at, retry in pending:
            if now - queued_at > self.PENDING_EVENT_TIMEOUT:
                continue
            try:
                retry()
            except Exception as e:
                logger.error(f"{self.plugin_id} - Error replaying queued event: {e}")

    def ping(self) -> bool:
        return True
