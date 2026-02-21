from __future__ import annotations

import json
import threading
from collections.abc import Callable
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def build_coinset_handler(
    on_event: Callable[[dict], None], expected_path: str = "/coinset/tx-block"
):
    class _Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:  # noqa: N802
            if self.path != expected_path:
                self.send_response(404)
                self.end_headers()
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
                body = self.rfile.read(length) if length > 0 else b"{}"
                payload = json.loads(body.decode("utf-8"))
                if not isinstance(payload, dict):
                    payload = {"raw": payload}
                on_event(payload)
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(b'{"ok":true}')
            except Exception:
                self.send_response(400)
                self.end_headers()

        def log_message(self, format: str, *args) -> None:  # noqa: A003
            return

    return _Handler


def start_coinset_webhook_server(
    listen_addr: str,
    on_event: Callable[[dict], None],
    path: str = "/coinset/tx-block",
) -> tuple[ThreadingHTTPServer, threading.Thread]:
    host, _, port_s = listen_addr.partition(":")
    port = int(port_s or "8787")
    server = ThreadingHTTPServer((host or "127.0.0.1", port), build_coinset_handler(on_event, path))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread
