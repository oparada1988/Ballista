from copy import deepcopy
import gi
gi.require_version("Gtk", "4.0")
from gi.repository import Gtk

from src.backend.PluginManager.ActionInputSupport import ActionInputSupport
from src.backend.DeckManagement.InputIdentifier import Input, InputIdentifier


class ActionHolder:
    def __init__(
        self,
        plugin_base,
        action_name: str,
        action_core=None,
        action_base=None,
        icon=None,
        min_app_version: str = None,
        action_id: str = None,
        action_id_suffix: str = None,
        action_support=None,
        description: str = None,
        requirements: str = None,
        settings_schema: dict = None,
        *args,
        **kwargs,
    ):
        self.plugin_base = plugin_base
        self.action_core = action_core if action_core is not None else action_base
        self.action_base = self.action_core  # backward compatibility alias
        self.action_id_suffix = action_id_suffix
        if action_id:
            self.action_id = action_id
        elif action_id_suffix:
            self.action_id = f"{plugin_base.get_plugin_id()}::{action_id_suffix}"
        else:
            cls_name = getattr(self.action_core, "__name__", action_name)
            self.action_id = f"{plugin_base.get_plugin_id()}::{cls_name}"
        self.action_name = action_name
        self.icon = icon if icon is not None else Gtk.Image(icon_name="insert-image-symbolic")
        self.min_app_version = min_app_version
        if action_support is None:
            self.action_support = {
                Input.Key: ActionInputSupport.UNTESTED,
                Input.Dial: ActionInputSupport.UNTESTED,
                Input.Touchscreen: ActionInputSupport.UNTESTED,
                Input.TouchKey: ActionInputSupport.UNTESTED,
                Input.Screen: ActionInputSupport.UNTESTED,
            }
        else:
            self.action_support = deepcopy(action_support)
        self.description = description
        self.requirements = requirements
        self.settings_schema = deepcopy(settings_schema) if settings_schema else None

    def get_is_compatible(self) -> bool:
        return True

    def init_and_get_action(self, deck_controller, page, state: int, input_ident: InputIdentifier):
        return self.action_core(
            action_id=self.action_id,
            action_name=self.action_name,
            deck_controller=deck_controller,
            page=page,
            input_ident=input_ident,
            plugin_base=self.plugin_base,
            state=state,
        )

    def get_input_compatibility(self, identifier: InputIdentifier):
        ident_type = type(identifier)
        if ident_type in self.action_support:
            return self.action_support[ident_type]
        if ident_type == Input.TouchKey and Input.Key in self.action_support:
            return self.action_support[Input.Key]
        if ident_type == Input.Screen and Input.Key in self.action_support:
            return self.action_support[Input.Key]
        return ActionInputSupport.UNSUPPORTED
