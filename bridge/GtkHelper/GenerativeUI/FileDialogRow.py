from GtkHelper.FileDialogRow import FileDialogRow as FileWidget
from GtkHelper.GenerativeUI.GenerativeUI import GenerativeUI


class FileDialogRow(GenerativeUI[str]):
    def __init__(
        self,
        action_core,
        var_name: str,
        default_value: str = "",
        title: str = None,
        subtitle: str = None,
        dialog_title: str = None,
        block_interaction: bool = True,
        only_show_filename: bool = False,
        filters=None,
        on_change=None,
        can_reset: bool = True,
        auto_add: bool = True,
        complex_var_name: bool = False,
    ):
        super().__init__(action_core, var_name, default_value, can_reset, auto_add, complex_var_name, on_change)
        self._widget = FileWidget(
            title=self.get_translation(title, title),
            subtitle=self.get_translation(subtitle, subtitle),
            dialog_title=dialog_title,
            initial_path=default_value,
            block_interaction=block_interaction,
            only_show_filename=only_show_filename,
            filters=filters,
            file_change_callback=self._on_file_changed,
        )

    def connect_signals(self):
        pass

    def disconnect_signals(self):
        pass

    def _on_file_changed(self, path: str):
        self._handle_value_changed(path)

    def set_ui_value(self, value: str):
        self._widget.load_from_path(str(value or ""))

    def set_file(self, path: str, update_setting: bool = False):
        self.set_ui_value(path)
        if update_setting:
            self.set_value(path)

    def get_file(self) -> str:
        return self._widget.selected_file_path
