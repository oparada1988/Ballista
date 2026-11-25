import os
import subprocess
import sys
import tempfile
import threading
import time
from loguru import logger

import gi
gi.require_version("GLib", "2.0")
from gi.repository import GLib

import globals as gl
from src.backend.DeckManagement.InputIdentifier import Input, InputEvent, InputIdentifier
from src.backend.PluginManager.EventManager import EventManager
from src.backend.PluginManager.EventAssigner import EventAssigner

try:
    from PIL import Image
except ImportError:
    Image = None

try:
    import rpyc
    from rpyc.utils.server import ThreadedServer
    _RPYC_BASE = rpyc.Service
except ImportError:
    rpyc = None
    ThreadedServer = None
    _RPYC_BASE = object

_MEDIA_CACHE_DIR = os.path.join(tempfile.gettempdir(), "ballista_media")
os.makedirs(_MEDIA_CACHE_DIR, exist_ok=True)


class ActionCore(_RPYC_BASE):
    backend_spawn_count = 0
    BACKEND_CONNECT_TIMEOUT: float = 30.0
    PENDING_EVENT_TIMEOUT: float = 30.0

    def __init__(
        self,
        action_id: str = "",
        action_name: str = "",
        deck_controller=None,
        page=None,
        plugin_base=None,
        state: int = 0,
        input_ident: InputIdentifier = None,
    ):
        self.backend_connection = None
        self.backend = None
        self.server = None
        self.backend_process = None
        self.backend_launch_pending: bool = False
        self._pending_action_events: list = []
        self._pending_action_events_lock = threading.Lock()

        self.action_id = action_id
        self.action_name = action_name
        self.deck_controller = deck_controller
        self.page = page
        self.plugin_base = plugin_base
        self.state = state
        self.input_ident = input_ident or Input.Key("0x0")

        self.generative_ui_objects: list = []
        self.on_ready_called: bool = False
        self.has_configuration: bool = False
        self.allow_event_configuration: bool = True
        self.put_custom_config_rows_below_gen_ui: bool = False

        # Official StreamController format: {"top": {"text": "...", ...}, ...}
        self.labels: dict = {}
        self.event_manager = EventManager()

        self._ballista_runner = getattr(deck_controller, "runner", None) if deck_controller else None
        self._ballista_slot = getattr(self, "_ballista_slot", None)
        self._ballista_settings: dict = {}
        self._media_seq: int = 0
        self._last_temp_media_file: str = None

    def clear_event_assigners(self):
        self.event_manager.clear_event_assigners()

    def load_event_overrides(self):
        self.event_manager.set_overrides(self.get_event_assignments())

    def set_deck_controller(self, deck_controller):
        self.deck_controller = deck_controller
        if deck_controller is not None:
            self._ballista_runner = getattr(deck_controller, "runner", self._ballista_runner)

    def set_page(self, page):
        self.page = page

    def get_input(self):
        if self.deck_controller is not None:
            return self.deck_controller.get_input(self.input_ident)
        return None

    def get_state(self):
        inp = self.get_input()
        if inp is None:
            return None
        return inp.states.get(self.state, inp.get_active_state())

    def get_own_key(self):
        return self.get_input()

    def add_event_assigner(self, event_assigner: EventAssigner):
        self.event_manager.add_event_assigner(event_assigner)

    def _raw_event_callback(self, event: InputEvent, data: dict = None):
        retry = lambda: self._raw_event_callback(event, data)
        if self.queue_action_event(retry):
            return
        if self.plugin_base is not None and self.plugin_base.queue_action_event(retry):
            return

        event_assigner = self.event_manager.get_event_assigner_for_event(event)
        if event_assigner is not None:
            event_assigner.call(data)
        else:
            # Fallback if a direct ActionCore subclass overrode event_callback without assigners
            self.event_callback(event, data)

    def event_callback(self, event: InputEvent, data: dict = None):
        pass

    def on_trigger(self):
        pass

    def on_tick(self):
        pass

    def on_ready(self):
        pass

    def on_update(self):
        self.on_ready()

    def on_labels_changed_in_ui(self):
        pass

    def get_is_multi_action(self) -> bool:
        return False

    def get_is_present(self) -> bool:
        if self._ballista_runner is not None and self._ballista_slot is not None:
            return self._ballista_runner.actions.get(self._ballista_slot) is self
        return True

    def get_own_action_index(self) -> int:
        return 0

    def has_image_control(self) -> bool:
        return True

    def has_background_control(self) -> bool:
        return True

    def has_label_control(self, label_index) -> bool:
        return True

    def has_label_controls(self) -> list:
        return [True, True, True]

    def has_custom_user_asset(self) -> bool:
        return False

    def raise_error_if_not_ready(self):
        # Permissive in Ballista compatibility bridge so early visual updates never throw
        pass

    def get_settings(self) -> dict:
        if self.page is not None:
            return self.page.get_action_settings(action_object=self)
        return self._ballista_settings

    def set_settings(self, settings: dict):
        self._ballista_settings = settings if settings is not None else {}
        if self._ballista_runner and self._ballista_slot is not None:
            self._ballista_runner.emit_set_settings(self._ballista_slot, self._ballista_settings)

    def connect(self, signal=None, callback=None):
        gl.signal_manager.connect_signal(signal=signal, callback=callback)

    def get_asset_path(self, asset_name: str, subdirs: list = None, asset_folder: str = "assets") -> str:
        if not self.plugin_base:
            return ""
        return self.plugin_base.get_asset_path(asset_name, subdirs=subdirs, asset_folder=asset_folder)

    def get_icon(self, key: str, skip_override: bool = False):
        if self.plugin_base and hasattr(self.plugin_base, "asset_manager"):
            return self.plugin_base.asset_manager.icons.get_asset(key, skip_override)
        return None

    def get_color(self, key: str, skip_override: bool = False):
        if self.plugin_base and hasattr(self.plugin_base, "asset_manager"):
            return self.plugin_base.asset_manager.colors.get_asset(key, skip_override)
        return None

    def get_translation(self, key: str, fallback: str = None) -> str:
        if self.plugin_base and hasattr(self.plugin_base, "locale_manager"):
            return self.plugin_base.locale_manager.get(key, fallback)
        return fallback if fallback is not None else key

    def set_background_color(self, color=None, update: bool = True):
        if color is not None and self._ballista_runner and self._ballista_slot is not None:
            self._ballista_runner.emit_set_background_color(self._ballista_slot, color)

    def show_error(self, duration: int = -1):
        if self._ballista_runner and self._ballista_slot is not None:
            self._ballista_runner.emit("show_error", {"slot": self._ballista_slot, "duration": duration})

    def hide_error(self):
        if self._ballista_runner and self._ballista_slot is not None:
            self._ballista_runner.emit("hide_error", {"slot": self._ballista_slot})

    def show_overlay(self, image, duration: int = -1):
        if image is not None:
            self.set_media(image=image)

    def hide_overlay(self):
        pass

    def _save_pil_image_to_cache(self, pil_image) -> str:
        self._media_seq += 1
        safe_slot = str(self._ballista_slot or "slot").replace("/", "_")
        out_path = os.path.join(_MEDIA_CACHE_DIR, f"{safe_slot}_{os.getpid()}_{self._media_seq}.png")
        tmp_path = f"{out_path}.tmp"
        try:
            pil_image.save(tmp_path, format="PNG")
            os.replace(tmp_path, out_path)
            # Clean up previous cached frame for this slot
            if self._last_temp_media_file and self._last_temp_media_file != out_path:
                try:
                    os.remove(self._last_temp_media_file)
                except OSError:
                    pass
            self._last_temp_media_file = out_path
            return out_path
        except Exception as e:
            logger.warning(f"Failed to serialize PIL image for {self._ballista_slot}: {e}")
            return ""

    def set_media(
        self,
        image=None,
        media_path=None,
        size: float = None,
        valign: float = None,
        halign: float = None,
        fps: int = 30,
        loop: bool = True,
        update: bool = True,
    ):
        resolved_path = None

        if media_path is not None:
            resolved_path = str(media_path)
        elif image is not None:
            if Image is not None and isinstance(image, Image.Image):
                resolved_path = self._save_pil_image_to_cache(image)
            elif isinstance(image, str):
                resolved_path = image
            elif hasattr(image, "get_file"):
                try:
                    f = image.get_file()
                    if f:
                        resolved_path = f.get_path()
                except Exception:
                    pass
            elif hasattr(image, "_path") and image._path:
                resolved_path = str(image._path)
            elif hasattr(image, "path") and image.path:
                resolved_path = str(image.path)
            elif hasattr(image, "save"):
                resolved_path = self._save_pil_image_to_cache(image)

        if self._ballista_runner and self._ballista_slot is not None:
            self._ballista_runner.emit_set_media(
                self._ballista_slot,
                resolved_path or "",
                size=size,
                valign=valign,
                halign=halign,
            )

    def set_label(
        self,
        text: str,
        position: str = "bottom",
        color=None,
        font_family: str = None,
        font_size=None,
        outline_width: int = None,
        outline_color=None,
        font_weight=None,
        font_style: str = None,
        update: bool = True,
    ):
        text_str = "" if text is None else str(text)
        self.labels[position] = {
            "text": text_str,
            "color": color,
            "font-family": font_family,
            "font-size": font_size,
            "outline_width": outline_width,
            "outline_color": outline_color,
            "font-weight": font_weight,
            "font-style": font_style,
        }
        if self._ballista_runner and self._ballista_slot is not None:
            self._ballista_runner.emit_set_label(
                self._ballista_slot,
                text_str,
                position=position,
                color=color,
                font_family=font_family,
                font_size=font_size,
                outline_width=outline_width,
                outline_color=outline_color,
            )

    def set_top_label(
        self,
        text: str,
        color=None,
        font_family: str = None,
        font_size=None,
        outline_width: int = None,
        outline_color=None,
        font_weight=None,
        font_style: str = None,
        update: bool = True,
    ):
        self.set_label(
            text, "top", color, font_family, font_size, outline_width, outline_color, font_weight, font_style, update
        )

    def set_center_label(
        self,
        text: str,
        color=None,
        font_family: str = None,
        font_size=None,
        outline_width: int = None,
        outline_color=None,
        font_weight=None,
        font_style: str = None,
        update: bool = True,
    ):
        self.set_label(
            text, "center", color, font_family, font_size, outline_width, outline_color, font_weight, font_style, update
        )

    def set_bottom_label(
        self,
        text: str,
        color=None,
        font_family: str = None,
        font_size=None,
        outline_width: int = None,
        outline_color=None,
        font_weight=None,
        font_style: str = None,
        update: bool = True,
    ):
        self.set_label(
            text, "bottom", color, font_family, font_size, outline_width, outline_color, font_weight, font_style, update
        )

    def set_state(self, state_index: int):
        self.state = state_index
        if self._ballista_runner and self._ballista_slot is not None:
            self._ballista_runner.emit_set_state(self._ballista_slot, state_index)

    def get_config_rows(self) -> list:
        return []

    def get_custom_config_area(self):
        return None

    def get_event_assignments(self) -> dict:
        if self.page:
            return self.page.get_action_event_assignments(action_object=self)
        return {}

    def get_page_event_assignments(self) -> dict:
        assignment = {}
        page_dict = self.get_event_assignments()
        for evt in Input.AllEvents():
            if evt.string_name in page_dict:
                assignment[evt] = Input.EventFromStringName(page_dict[evt.string_name])
            else:
                assignment[evt] = evt
        return assignment

    def set_event_assignment(self, input_event, event_assigner):
        if self.page:
            self.page.set_action_event_assigment(event_assigner=event_assigner, input_event=input_event, action_object=self)
        self.load_event_overrides()

    def set_all_events_to_null(self):
        for evt in self.event_manager.get_event_map().keys():
            self.set_event_assignment(evt, None)

    # ---------- Generative UI ---------- #
    def get_generative_ui_objects(self) -> list:
        return list(self.generative_ui_objects)

    def add_generative_ui_object(self, generative_ui_object):
        if generative_ui_object not in self.generative_ui_objects:
            self.generative_ui_objects.append(generative_ui_object)

    def get_generative_ui(self) -> list:
        return self.generative_ui_objects

    def get_generative_ui_widgets(self) -> list:
        widgets = []
        for obj in self.generative_ui_objects:
            if getattr(obj, "auto_add", True) and getattr(obj, "widget", None) is not None:
                widgets.append(obj.widget)
        return widgets

    def load_initial_generative_ui(self):
        GLib.idle_add(self._do_load_initial_generative_ui)

    def _do_load_initial_generative_ui(self):
        for obj in self.generative_ui_objects:
            try:
                obj.load_initial_ui()
            except Exception as e:
                logger.warning(f"Error loading initial generative UI for {self.action_id}: {e}")
        return False

    # ---------- RPyC Action Backend Support ---------- #
    def start_server(self):
        if rpyc is None or ThreadedServer is None:
            return
        if self.server is not None:
            return
        self.server = ThreadedServer(self, hostname="localhost", port=0, protocol_config={"allow_public_attrs": True})
        threading.Thread(target=self.server.start, name=f"rpyc_act_{self.action_id}", daemon=True).start()

    def on_disconnect(self):
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
            try:
                self.backend_process.terminate()
            except Exception:
                pass
        self.server = None
        self.backend = None
        self.backend_connection = None
        self.backend_process = None
        self.backend_launch_pending = False

    def launch_backend(self, backend_path: str, venv_path: str = None, open_in_terminal: bool = False):
        if self.backend_process is not None and self.backend_process.poll() is None:
            return
        if rpyc is None:
            return
        if not os.path.exists(backend_path):
            return
        if venv_path is not None and self.plugin_base and not self.plugin_base.is_backend_venv_healthy(venv_path):
            self.plugin_base.recreate_venv(self.plugin_base.PATH)
        self.start_server()
        if self.server is None:
            return
        port = self.server.port
        env = os.environ.copy()
        env["PYTHONPATH"] = f"{gl.top_level_dir}:{env.get('PYTHONPATH', '')}".rstrip(":")
        python_bin = os.path.join(venv_path, "bin", "python3") if (venv_path and os.path.isfile(os.path.join(venv_path, "bin", "python3"))) else sys.executable
        self.backend_launch_pending = True
        self.backend_process = subprocess.Popen([python_bin, backend_path, f"--port={port}"], env=env, start_new_session=open_in_terminal)
        ActionCore.backend_spawn_count += 1
        threading.Thread(target=self.wait_for_backend, daemon=True).start()

    def wait_for_backend(self, timeout: float = None):
        if timeout is None:
            timeout = self.BACKEND_CONNECT_TIMEOUT
        deadline = time.monotonic() + timeout
        while self.backend_connection is None and time.monotonic() < deadline:
            time.sleep(0.1)
        if self.backend_connection is None:
            self.backend_launch_pending = False
            self._flush_pending_action_events()

    def register_backend(self, port: int):
        if rpyc is None:
            return
        self.backend_connection = rpyc.connect("localhost", port, config={"allow_public_attrs": True})
        self.backend = self.backend_connection.root
        gl.plugin_manager.backends.append(self.backend_connection)
        self.backend_launch_pending = False
        self.on_backend_ready()
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
                logger.error(f"{self.action_id} - Error replaying queued action event: {e}")

    def on_backend_ready(self):
        pass

    def ping(self) -> bool:
        return True

    def on_removed_from_cache(self):
        self.on_disconnect()
        if self._last_temp_media_file:
            try:
                os.remove(self._last_temp_media_file)
            except OSError:
                pass
            self._last_temp_media_file = None

    def on_remove(self):
        self.on_removed_from_cache()
