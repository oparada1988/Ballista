class Observer:
    def __init__(self):
        self._listeners: list = []

    def subscribe(self, callback):
        if callback and callback not in self._listeners:
            self._listeners.append(callback)

    def unsubscribe(self, callback):
        if callback in self._listeners:
            self._listeners.remove(callback)

    def notify(self, *args, **kwargs):
        for cb in list(self._listeners):
            try:
                cb(*args, **kwargs)
            except Exception:
                pass
