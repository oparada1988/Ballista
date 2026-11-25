import functools
from abc import ABC, abstractmethod
from typing import TypeVar, Callable, Generic

T = TypeVar("T")


class GenerativeUI(Generic[T], ABC):
    def __init__(
        self,
        action_core,
        var_name: str,
        default_value: T,
        can_reset: bool = True,
        auto_add: bool = True,
        complex_var_name: bool = False,
        on_change: Callable = None,
    ):
        self._action_core = action_core
        self._var_name = var_name
        self._default_value = default_value
        self.on_change = on_change
        self._can_reset = can_reset
        self._auto_add = auto_add
        self._complex_var_name = complex_var_name
        self._widget = None
        if action_core is not None and hasattr(action_core, "add_generative_ui_object"):
            action_core.add_generative_ui_object(self)

    @abstractmethod
    def connect_signals(self):
        pass

    @abstractmethod
    def disconnect_signals(self):
        pass

    @property
    def action_core(self):
        return self._action_core

    @property
    def var_name(self):
        return self._var_name

    @property
    def default_value(self):
        return self._default_value

    @property
    def widget(self):
        return self._widget

    @property
    def can_reset(self):
        return self._can_reset

    @property
    def auto_add(self):
        return self._auto_add

    @property
    def complex_var_name(self):
        return self._complex_var_name

    @staticmethod
    def signal_manager(func):
        @functools.wraps(func)
        def wrapper(self, *args, **kwargs):
            self.disconnect_signals()
            try:
                return func(self, *args, **kwargs)
            finally:
                self.connect_signals()
        return wrapper

    @abstractmethod
    def set_ui_value(self, value: T):
        pass

    def _handle_value_changed(self, new_value: T, update_settings: bool = True, trigger_callback: bool = True):
        old_value = self.get_value()
        if update_settings:
            self.set_value(new_value)
        if trigger_callback and self.on_change:
            self.on_change(self.widget, new_value, old_value)

    def update_value_in_ui(self):
        self.set_ui_value(self.get_value())

    def reset_value(self):
        self._handle_value_changed(self._default_value)
        self.update_value_in_ui()

    def resolve_var_name(self) -> list:
        if self._complex_var_name:
            return self._var_name.split(".")
        return [self._var_name]

    def set_value(self, value: T):
        settings = self._action_core.get_settings()
        keys = self.resolve_var_name()
        d = settings
        for k in keys[:-1]:
            if k not in d or not isinstance(d[k], dict):
                d[k] = {}
            d = d[k]
        d[keys[-1]] = value
        self._action_core.set_settings(settings)

    def get_value(self, fallback: T = None) -> T:
        settings = self._action_core.get_settings()
        keys = self.resolve_var_name()
        d = settings
        for k in keys:
            if not isinstance(d, dict) or k not in d:
                return fallback if fallback is not None else self._default_value
            d = d[k]
        return d

    def load_initial_ui(self):
        value = self.get_value()
        self.set_ui_value(value)
        self._handle_value_changed(value, False)

    def load_ui_value(self):
        self.set_ui_value(self.get_value())

    def get_translation(self, key: str, fallback: str = None) -> str:
        if not key:
            return ""
        return self._action_core.get_translation(key, fallback)

    def set_sensitive(self, sensitive: bool):
        if self._widget:
            self._widget.set_sensitive(sensitive)

    def get_sensitive(self) -> bool:
        return self._widget.get_sensitive() if self._widget else True

    def set_visible(self, visible: bool):
        if self._widget:
            self._widget.set_visible(visible)

    def get_visible(self) -> bool:
        return self._widget.get_visible() if self._widget else True

    def unparent(self):
        if self._widget and self._widget.get_parent():
            self._widget.unparent()
