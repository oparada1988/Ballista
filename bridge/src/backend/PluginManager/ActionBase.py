from src.backend.DeckManagement.InputIdentifier import Input, InputEvent, InputIdentifier
from src.backend.PluginManager.ActionCore import ActionCore
from src.backend.PluginManager.EventAssigner import EventAssigner


class ActionBase(ActionCore):
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
        super().__init__(action_id, action_name, deck_controller, page, plugin_base, state, input_ident)

        # Register standard backward-compatible event assigners matching official StreamController
        for evt in (
            Input.Key.Events.DOWN,
            Input.Key.Events.UP,
            Input.Key.Events.SHORT_UP,
            Input.Key.Events.HOLD_START,
            Input.Key.Events.HOLD_STOP,
            Input.Dial.Events.DOWN,
            Input.Dial.Events.UP,
            Input.Dial.Events.SHORT_UP,
            Input.Dial.Events.HOLD_START,
            Input.Dial.Events.HOLD_STOP,
            Input.Dial.Events.TURN_CW,
            Input.Dial.Events.TURN_CCW,
            Input.Dial.Events.SHORT_TOUCH_PRESS,
            Input.Dial.Events.LONG_TOUCH_PRESS,
            Input.Touchscreen.Events.DRAG_LEFT,
            Input.Touchscreen.Events.DRAG_RIGHT,
            Input.Touchscreen.Events.SHORT_TOUCH_PRESS,
            Input.Touchscreen.Events.LONG_TOUCH_PRESS,
        ):
            self.add_event_assigner(
                EventAssigner(
                    id=evt.string_name,
                    ui_label=evt.string_name,
                    default_events=[evt],
                    callback=lambda data, _e=evt: self.event_callback(_e, data),
                )
            )

    def event_callback(self, event: InputEvent, data: dict = None):
        if event in (Input.Key.Events.DOWN, Input.Dial.Events.DOWN):
            self.on_key_down()
        elif event in (Input.Key.Events.UP, Input.Dial.Events.UP):
            self.on_key_up()

    def on_key_down(self):
        pass

    def on_key_up(self):
        pass
