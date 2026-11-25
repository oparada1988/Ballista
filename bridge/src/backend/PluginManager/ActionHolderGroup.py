from src.backend.PluginManager.ActionHolder import ActionHolder


class ActionHolderGroup:
    def __init__(self, group_name: str = "", action_holders: list = None):
        self.group_name = group_name
        self.action_holders: list[ActionHolder] = list(action_holders) if action_holders else []

    def add_action_holder(self, holder: ActionHolder):
        if holder not in self.action_holders:
            self.action_holders.append(holder)

    def get_action_holders(self) -> list[ActionHolder]:
        return list(self.action_holders)
