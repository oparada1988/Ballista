class Page:
    def __init__(self, deck_controller=None):
        self.deck_controller = deck_controller
        self.dict = {}
        self.action_objects = {}
        self.json_path = ""

    def get_action_settings(self, action_object=None) -> dict:
        if action_object and hasattr(action_object, "_ballista_settings"):
            return action_object._ballista_settings
        return {}

    def set_action_settings(self, action_object=None, settings: dict = None):
        if action_object is not None:
            action_object._ballista_settings = settings if settings is not None else {}
            if hasattr(action_object, "_ballista_runner") and action_object._ballista_runner and action_object._ballista_slot is not None:
                action_object._ballista_runner.emit_set_settings(action_object._ballista_slot, action_object._ballista_settings)

    def get_all_actions(self) -> list:
        if self.deck_controller and self.deck_controller.runner:
            return list(self.deck_controller.runner.actions.values())
        return []

    def get_all_actions_for_input(self, input_ident, state=0) -> list:
        matches = []
        for act in self.get_all_actions():
            if getattr(act, "input_ident", None) == input_ident:
                matches.append(act)
        return matches

    def get_action_event_assignments(self, action_object=None) -> dict:
        return getattr(action_object, "_event_assignments", {}) if action_object else {}

    def set_action_event_assigment(self, event_assigner=None, input_event=None, action_object=None):
        if action_object is not None and input_event is not None:
            if not hasattr(action_object, "_event_assignments"):
                action_object._event_assignments = {}
            action_object._event_assignments[str(input_event)] = event_assigner.id if event_assigner else None

    def save(self):
        pass
