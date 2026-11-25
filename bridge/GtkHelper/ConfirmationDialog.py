import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw


class ConfirmationDialog(Adw.MessageDialog):
    def __init__(self, title: str = "", body: str = "", confirm: str = "Confirm", transient_for=None, on_cancel=None, on_confirm=None):
        super().__init__(heading=title, body=body, transient_for=transient_for)
        self.on_cancel = on_cancel
        self.on_confirm = on_confirm
        self.add_response("cancel", "Cancel")
        self.add_response("confirm", confirm)
        self.connect("response", self.on_response)

    def on_response(self, dialog, response):
        if response == "confirm" and self.on_confirm:
            self.on_confirm()
        elif self.on_cancel:
            self.on_cancel()
        dialog.destroy()
