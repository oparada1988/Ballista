class EventAssigner:
    def __init__(self, id: str, ui_label: str, callback, default_events: list = None, default_event=None, tooltip: str = None):
        self.id = id
        self.ui_label = ui_label
        if default_events is not None:
            self.default_events = list(default_events)
        elif default_event is not None:
            self.default_events = [default_event]
        else:
            self.default_events = []
        self.callback = callback
        self.tooltip = tooltip

    def call(self, *args, **kwargs):
        if self.callback is not None:
            return self.callback(*args, **kwargs)
