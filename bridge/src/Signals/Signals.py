class Signal:
    pass


class PageRename(Signal):
    pass


class PageDelete(Signal):
    pass


class PageAdd(Signal):
    pass


class ChangePage(Signal):
    pass


class PluginInstall(Signal):
    pass


class AppQuit(Signal):
    pass


class RemoveState(Signal):
    pass
