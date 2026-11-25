class BackgroundImage:
    def __init__(self, deck_controller=None, media_path: str = None, fps: int = 30, loop: bool = True):
        self.deck_controller = deck_controller
        self.media_path = media_path
        self.fps = fps
        self.loop = loop


class ScreenSaver:
    def __init__(self):
        self.showing = False

    def show(self):
        self.showing = True

    def hide(self):
        self.showing = False


class _LabelManager:
    def set_action_label(self, label=None, position="bottom", update=True):
        pass


class _LayoutManager:
    def set_action_layout(self, layout=None, update=True):
        pass


class _BackgroundManager:
    def set_action_color(self, color=None):
        pass


class _ActionPermissionManager:
    def __init__(self):
        self._img_idx = 0
        self._bg_idx = 0
        self._label_indices = [0, 0, 0]

    def get_image_control_index(self):
        return self._img_idx

    def set_image_control_index(self, idx, reload_pages=False, reload_self=False):
        self._img_idx = idx

    def get_background_control_index(self):
        return self._bg_idx

    def set_background_control_index(self, idx, reload_pages=False, reload_self=False):
        self._bg_idx = idx

    def get_label_control_index(self, label_idx):
        if 0 <= label_idx < len(self._label_indices):
            return self._label_indices[label_idx]
        return 0

    def set_label_control_index(self, label_idx, idx, reload_pages=False, reload_self=False):
        if 0 <= label_idx < len(self._label_indices):
            self._label_indices[label_idx] = idx

    def get_label_control_indices(self):
        return list(self._label_indices)


class ControllerInputState:
    def __init__(self, controller_input=None, state: int = 0):
        self.controller_input = controller_input
        self.state = state
        self.action_permission_manager = _ActionPermissionManager()
        self.label_manager = _LabelManager()
        self.layout_manager = _LayoutManager()
        self.background_manager = _BackgroundManager()

    def set_image(self, image=None, update=True):
        pass

    def set_video(self, video=None, update=True):
        pass

    def show_error(self, duration: int = -1):
        pass

    def hide_error(self):
        pass

    def show_overlay(self, image, duration: int = -1):
        pass

    def hide_overlay(self):
        pass


class ControllerInput:
    def __init__(self, deck_controller=None, identifier=None):
        self.deck_controller = deck_controller
        self.identifier = identifier
        self.state = 0
        self.states = {0: ControllerInputState(self, 0)}

    def get_active_state(self):
        return self.states.get(self.state, self.states[0])

    def set_state(self, state: int, update_sidebar: bool = False):
        self.state = state
        if state not in self.states:
            self.states[state] = ControllerInputState(self, state)

    def update(self):
        pass


class ControllerKey(ControllerInput):
    pass


class ControllerDial(ControllerInput):
    pass


class ControllerTouchScreen(ControllerInput):
    pass


class MockDeck:
    KEY_COUNT = 8

    def __init__(self, rows: int = 2, cols: int = 4, serial: str = "BALLISTA-DECK"):
        self._rows = rows
        self._cols = cols
        self._serial = serial

    def key_layout(self):
        return (self._rows, self._cols)

    def get_serial_number(self) -> str:
        return self._serial

    def deck_type(self) -> str:
        return "Stream Deck +"

    def is_visual(self) -> bool:
        return True

    def key_image_format(self) -> dict:
        return {"size": (120, 120), "format": "JPEG", "flip": (False, False), "rotation": 0}


class DeckController:
    def __init__(self, runner=None):
        self.runner = runner
        self.deck = MockDeck()
        self.serial_number = self.deck.get_serial_number()
        self.active_page = None
        self.keys = {}
        self.dials = {}
        self.touchscreens = {}
        self._inputs = {}
        self.screen_saver = ScreenSaver()
        self.brightness = 80

    def get_input(self, input_ident):
        if input_ident is None:
            return None
        key = str(input_ident)
        if key not in self._inputs:
            self._inputs[key] = ControllerInput(self, input_ident)
        return self._inputs[key]

    def get_page_for_input(self, input_ident):
        return self.active_page

    def load_page(self, page, load_brightness: bool = True, load_screensaver: bool = True, load_background: bool = True, allow_reload: bool = True):
        self.active_page = page

    def set_brightness(self, value: int):
        self.brightness = max(0, min(100, int(value)))
        if self.runner:
            self.runner.emit("set_brightness", {"brightness": self.brightness})

    def get_brightness(self) -> int:
        return self.brightness
