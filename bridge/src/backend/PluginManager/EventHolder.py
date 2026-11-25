class EventHolder:
    def __init__(self, plugin_base=None, event_id: str = None, event_id_suffix: str = None, id: str = None, name: str = "", callback=None):
        self.plugin_base = plugin_base
        self.event_id_suffix = event_id_suffix
        if event_id:
            self.event_id = event_id
        elif id:
            self.event_id = id
        elif plugin_base and event_id_suffix:
            self.event_id = f"{plugin_base.get_plugin_id()}::{event_id_suffix}"
        else:
            self.event_id = event_id_suffix or name
        self.id = self.event_id
        self.name = name
        self._listeners: list = []
        if callback:
            self._listeners.append(callback)

    def add_listener(self, callback):
        if callback and callback not in self._listeners:
            self._listeners.append(callback)

    def remove_listener(self, callback):
        if callback in self._listeners:
            self._listeners.remove(callback)

    def trigger_event(self, *args, **kwargs):
        for cb in list(self._listeners):
            try:
                cb(*args, **kwargs)
            except Exception:
                pass
