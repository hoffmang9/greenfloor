"""Thread-local HTTP stub for Dexie API used by Rust DexieClient in daemon tests."""

from __future__ import annotations

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any
from urllib.parse import unquote, urlparse


class DexieHttpMock:
    def __init__(self) -> None:
        self._offers: dict[str, dict[str, Any]] = {}
        self._list_offers: list[dict[str, Any]] = []
        self._cancel_failures: dict[str, str] = {}
        self._server: ThreadingHTTPServer | None = None
        self._thread: threading.Thread | None = None
        self.base_url = "http://127.0.0.1:0"

    def set_cancel_failure(self, offer_id: str, error: str) -> None:
        self._cancel_failures[str(offer_id)] = str(error)

    def set_offers(self, offers: dict[str, dict[str, Any]]) -> None:
        self._offers = {str(key): dict(value) for key, value in offers.items()}
        self._list_offers = [dict(value) for value in self._offers.values()]

    def start(self) -> str:
        mock = self

        class _Handler(BaseHTTPRequestHandler):
            def log_message(self, _format: str, *_args) -> None:
                return

            def do_GET(self) -> None:
                parsed = urlparse(self.path)
                if parsed.path == "/v1/offers":
                    body = json.dumps({"success": True, "offers": mock._list_offers}).encode()
                    self._write(200, body)
                    return
                if parsed.path.startswith("/v1/offers/"):
                    offer_id = unquote(parsed.path.rsplit("/", 1)[-1])
                    row = mock._offers.get(offer_id)
                    if row is None:
                        self._write(404, b'{"success":false,"error":"not_found"}')
                        return
                    body = json.dumps({"success": True, "offer": row}).encode()
                    self._write(200, body)
                    return
                self._write(404, b'{"success":false}')

            def do_POST(self) -> None:
                parsed = urlparse(self.path)
                length = int(self.headers.get("Content-Length", "0"))
                raw = self.rfile.read(length) if length else b"{}"
                payload = json.loads(raw.decode("utf-8") or "{}")
                if parsed.path.startswith("/v1/offers/") and parsed.path.endswith("/cancel"):
                    offer_id = unquote(parsed.path.split("/")[3])
                    failure = mock._cancel_failures.get(offer_id)
                    if failure is not None:
                        body = json.dumps({"success": False, "error": failure}).encode()
                        self._write(200, body)
                        return
                    row = mock._offers.setdefault(offer_id, {"id": offer_id, "status": 0})
                    row["status"] = 3
                    body = json.dumps({"success": True, "id": offer_id, "status": 3}).encode()
                    self._write(200, body)
                    return
                if parsed.path == "/v1/offers":
                    offer_id = str(payload.get("id") or "offer-1")
                    row = {"id": offer_id, "status": 0, **payload}
                    mock._offers[offer_id] = row
                    mock._list_offers = list(mock._offers.values())
                    body = json.dumps({"success": True, "id": offer_id}).encode()
                    self._write(200, body)
                    return
                self._write(404, b'{"success":false}')

            def _write(self, status: int, body: bytes) -> None:
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), _Handler)
        host, port, *_ = self._server.server_address
        self.base_url = f"http://{host}:{port}"
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()
        return self.base_url

    def stop(self) -> None:
        if self._server is not None:
            self._server.shutdown()
            self._server.server_close()
            self._server = None
        if self._thread is not None:
            self._thread.join(timeout=2)
            self._thread = None

    def snapshot_offers(self) -> dict[str, dict[str, Any]]:
        return {key: dict(value) for key, value in self._offers.items()}
