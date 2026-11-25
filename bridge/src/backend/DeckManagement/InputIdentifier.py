from enum import Enum


class InputIdentifier:
    def __init__(self, input_type: str, json_identifier: str, controller_class_name: str = "ControllerInput"):
        self.input_type = input_type
        self.json_identifier = str(json_identifier)
        self.controller_class_name = controller_class_name

    def get_config(self, page=None) -> dict:
        if page is None or not hasattr(page, "dict"):
            return {}
        return page.dict.get(self.input_type, {}).get(self.json_identifier, {})

    def get_dict(self, d: dict):
        return d.get(self.input_type, {}).get(self.json_identifier)

    def get_controller_input(self, controller):
        return controller.get_input(self) if controller else None

    def __eq__(self, other):
        if other is None:
            return False
        if not isinstance(other, InputIdentifier):
            return False
        return self.input_type == other.input_type and self.json_identifier == other.json_identifier

    def __hash__(self) -> int:
        return hash((self.input_type, self.json_identifier))

    def __str__(self) -> str:
        return f"Input({self.input_type}, {self.json_identifier})"


class InputEvent(Enum):
    def __new__(cls, string_name):
        obj = object.__new__(cls)
        obj.string_name = string_name
        return obj

    def __str__(self) -> str:
        return self.string_name


class Input:
    class Key(InputIdentifier):
        input_type = "keys"
        controller_class_name = "ControllerKey"

        class Events(InputEvent):
            DOWN = "Key Down"
            UP = "Key Up"
            SHORT_UP = "Key Short Up"
            HOLD_START = "Key Hold Start"
            HOLD_STOP = "Key Hold Stop"

        def __init__(self, json_identifier: str = "0x0"):
            raw = str(json_identifier)
            if "x" in raw:
                self.coords = Input.Key.Coords_From_PageCoords(raw)
                self.json_identifier = Input.Key.Coords_To_PageCoords(self.coords)
                self._raw_index = None
            else:
                try:
                    idx = int(raw)
                except ValueError:
                    idx = 0
                self._raw_index = idx
                self.coords = (idx % 4, idx // 4)
                self.json_identifier = Input.Key.Coords_To_PageCoords(self.coords)
            super().__init__(self.input_type, self.json_identifier, self.controller_class_name)

        @staticmethod
        def Coords_From_PageCoords(page_coords: str):
            split = str(page_coords).split("x")
            if len(split) == 2:
                return (int(split[0]), int(split[1]))
            idx = int(split[0]) if split[0].isdigit() else 0
            return (idx % 4, idx // 4)

        @staticmethod
        def Coords_To_PageCoords(coords: tuple):
            return f"{coords[0]}x{coords[1]}"

        @staticmethod
        def Index_To_Coords(deck_controller, index: int):
            rows, cols = deck_controller.deck.key_layout()
            x = index % cols
            y = index // cols
            return (x, y)

        @staticmethod
        def Coords_To_Index(deck_controller, coords):
            if isinstance(coords, str):
                coords = coords.split("x")
            x, y = map(int, coords)
            rows, cols = deck_controller.deck.key_layout()
            return y * cols + x

        def get_page_coords(self) -> str:
            return self.Coords_To_PageCoords(self.coords)

        def get_index(self, deck_controller=None) -> int:
            if self._raw_index is not None:
                return self._raw_index
            if deck_controller is not None:
                return self.Coords_To_Index(deck_controller, self.coords)
            return self.coords[1] * 4 + self.coords[0]

    class Dial(InputIdentifier):
        input_type = "dials"
        controller_class_name = "ControllerDial"

        class Events(InputEvent):
            DOWN = "Dial Down"
            UP = "Dial Up"
            SHORT_UP = "Dial Short Up"
            HOLD_START = "Dial Hold Start"
            HOLD_STOP = "Dial Hold Stop"
            TURN_CW = "Dial Turn CW"
            TURN_CCW = "Dial Turn CCW"
            SHORT_TOUCH_PRESS = "Dial Touchscreen Short Press"
            LONG_TOUCH_PRESS = "Dial Touchscreen Long Press"

        def __init__(self, json_identifier: str = "0"):
            self.index = int(json_identifier) if str(json_identifier).isdigit() else 0
            super().__init__(self.input_type, str(json_identifier), self.controller_class_name)

    class Touchscreen(InputIdentifier):
        input_type = "touchscreens"
        controller_class_name = "ControllerTouchScreen"

        class Events(InputEvent):
            DRAG_LEFT = "Touchscreen Drag Left"
            DRAG_RIGHT = "Touchscreen Drag Right"
            SHORT_TOUCH_PRESS = "Touchscreen Short Press"
            LONG_TOUCH_PRESS = "Touchscreen Long Press"

        def __init__(self, json_identifier: str = "0"):
            self.index = str(json_identifier)
            super().__init__(self.input_type, str(json_identifier), self.controller_class_name)

    class TouchKey(InputIdentifier):
        input_type = "touch_keys"
        controller_class_name = "ControllerTouchKey"

        def __init__(self, json_identifier: str = "0"):
            self.index = int(json_identifier) if str(json_identifier).isdigit() else 0
            super().__init__(self.input_type, str(json_identifier), self.controller_class_name)

    TouchKey.Events = Key.Events

    class Screen(InputIdentifier):
        input_type = "screens"
        controller_class_name = "ControllerScreen"

        class Events(InputEvent):
            UPDATE = "Screen Update"

        def __init__(self, json_identifier: str = "0"):
            self.index = str(json_identifier)
            super().__init__(self.input_type, str(json_identifier), self.controller_class_name)

    All = (Key, Dial, Touchscreen, TouchKey, Screen)
    KeyTypes = [kt.input_type for kt in All]

    @staticmethod
    def FromTypeIdentifier(input_type: str, json_identifier: str):
        input_map = {
            "keys": Input.Key,
            "dials": Input.Dial,
            "touchscreens": Input.Touchscreen,
            "touch_keys": Input.TouchKey,
            "screens": Input.Screen,
        }
        if input_type in input_map:
            return input_map[input_type](json_identifier)
        raise ValueError(f"Unknown input type {input_type}")

    @staticmethod
    def AllEvents() -> list:
        events = []
        for t in Input.All:
            for evt in t.Events:
                if evt not in events:
                    events.append(evt)
        return events

    @staticmethod
    def EventFromStringName(string_name: str):
        if string_name in [None, str(None)]:
            return None
        for evt in Input.AllEvents():
            if evt.string_name == string_name:
                return evt
        return None
