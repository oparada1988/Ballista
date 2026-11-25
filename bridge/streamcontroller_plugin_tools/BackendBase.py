import argparse
import sys
import threading
from loguru import logger as log

try:
    import rpyc
    from rpyc.utils.server import ThreadedServer
    _RPYC_BASE = rpyc.Service
except ImportError:
    rpyc = None
    ThreadedServer = None
    _RPYC_BASE = object


class BackendBase(_RPYC_BASE):
    def __init__(self):
        self.frontend_connection = None
        self.frontend = None
        self.server = None
        if rpyc is not None:
            self.connect_to_frontend()
            self.start_server()
            self.register_to_frontend()

    def connect_to_frontend(self):
        port = int(self.get_args().port)
        self.frontend_connection = rpyc.connect("localhost", port, config={"allow_public_attrs": True})
        self.frontend = self.frontend_connection.root

    def start_server(self):
        self.server = ThreadedServer(self, port=0, protocol_config={"allow_public_attrs": True})
        threading.Thread(target=self.server.start, name="backend_server_start", daemon=False).start()
        log.success("Started backend RPyC server")

    def register_to_frontend(self):
        self.frontend.register_backend(port=self.server.port)

    def on_disconnect(self, conn):
        log.info("Backend RPyC connection closed")
        if self.server is not None:
            self.server.close()
        if self.frontend_connection is not None:
            self.frontend_connection.close()

    def get_args(self):
        parser = argparse.ArgumentParser(prog="BackendBase")
        parser.add_argument("--port", type=str)
        args = parser.parse_args()
        if args.port is None:
            parser.print_help()
            sys.exit(1)
        return args
