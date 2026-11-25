from src.backend.DeckManagement.InputIdentifier import Input, InputEvent
from src.backend.PluginManager.EventAssigner import EventAssigner


class EventManager:
    def __init__(self):
        self._event_assigners: list[EventAssigner] = []
        self._overrides: dict[str, str] = {}

    def set_overrides(self, overrides: dict):
        self._overrides = overrides or {}

    def add_event_assigner(self, event_assigner: EventAssigner):
        existing = self.get_event_assigner_by_id(event_assigner.id)
        if existing is not None:
            self._event_assigners.remove(existing)
        self._event_assigners.append(event_assigner)

    def clear_event_assigners(self):
        self._event_assigners.clear()

    def get_all_event_assigners(self) -> list[EventAssigner]:
        return list(self._event_assigners)

    def get_event_assigner_by_id(self, assigner_id: str):
        for assigner in self._event_assigners:
            if assigner.id == assigner_id:
                return assigner
        return None

    def get_event_map(self, ignore_overrides: bool = False) -> dict:
        event_map = {}
        for event in Input.AllEvents():
            event_map[event] = None

        for assigner in self._event_assigners:
            for event in assigner.default_events:
                if event is not None:
                    event_map[event] = assigner

        if not ignore_overrides:
            for input_event_str, event_id in self._overrides.items():
                input_event = Input.EventFromStringName(input_event_str)
                if input_event is not None:
                    assigner = self.get_event_assigner_by_id(event_id) if event_id else None
                    event_map[input_event] = assigner

        return event_map

    def get_event_assigner_for_event(self, event: InputEvent):
        return self.get_event_map().get(event)
