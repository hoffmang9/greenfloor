"""Local HTTP stand-in for Coinset MSP lookup_asset_by_symbol."""

from __future__ import annotations

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class _MspLookupHandler(BaseHTTPRequestHandler):
    symbol_assets: dict[str, str] = {}
    protocol_version = "HTTP/1.1"

    def log_message(self, _format: str, *_args: object) -> None:
        return

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length", "0"))
        body = json.loads(self.rfile.read(length).decode("utf-8"))
        if self.path.rstrip("/").endswith("lookup_asset_by_symbol"):
            symbol = str(body.get("symbol", "")).strip().upper()
            asset_id = self.symbol_assets.get(symbol)
            if asset_id is None:
                payload = {"success": False}
            else:
                payload = {
                    "success": True,
                    "asset": {"asset_id": asset_id, "symbol": symbol},
                }
        else:
            payload = {"success": False}
        encoded = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(encoded)
        self.wfile.flush()
        self.close_connection = True


class _ReusableThreadingHTTPServer(ThreadingHTTPServer):
    allow_reuse_address = True
    daemon_threads = True


class MspLookupMockServer:
    """Threaded MSP mock exposing lookup_asset_by_symbol for integration tests."""

    def __init__(self, *, symbol_assets: dict[str, str]) -> None:
        self._symbol_assets = {key.upper(): value.lower() for key, value in symbol_assets.items()}
        self._server = _ReusableThreadingHTTPServer(("127.0.0.1", 0), _MspLookupHandler)
        _MspLookupHandler.symbol_assets = self._symbol_assets
        self.base_url = f"http://127.0.0.1:{self._server.server_address[1]}"
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    def __enter__(self) -> MspLookupMockServer:
        self._thread.start()
        return self

    def __exit__(self, *_exc: object) -> None:
        self._server.shutdown()
        self._thread.join(timeout=5)


def write_signer_program_yaml(
    path,
    *,
    home_dir: str,
    msp_base_url: str,
) -> None:
    path.write_text(
        "\n".join(
            [
                "app:",
                '  network: "mainnet"',
                f'  home_dir: "{home_dir}"',
                "runtime:",
                "  loop_interval_seconds: 30",
                "chain_signals:",
                "  tx_block_trigger:",
                "    webhook_enabled: true",
                '    webhook_listen_addr: "127.0.0.1:8787"',
                "dev:",
                "  python:",
                '    min_version: "3.11"',
                "notifications:",
                "  low_inventory_alerts:",
                "    enabled: true",
                '    threshold_mode: "absolute_base_units"',
                "    default_threshold_base_units: 0",
                "    dedup_cooldown_seconds: 60",
                "    clear_hysteresis_percent: 10",
                "  providers:",
                "    - type: pushover",
                "      enabled: true",
                '      user_key_env: "PUSHOVER_USER_KEY"',
                '      app_token_env: "PUSHOVER_APP_TOKEN"',
                '      recipient_key_env: "PUSHOVER_RECIPIENT_KEY"',
                "venues:",
                "  dexie:",
                '    api_base: "https://api.dexie.space"',
                "  splash:",
                '    api_base: "http://localhost:4000"',
                "  offer_publish:",
                '    provider: "dexie"',
                "signer:",
                '  kms_key_id: "arn:aws:kms:us-west-2:123:key/demo"',
                '  kms_region: "us-west-2"',
                '  kms_public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"',
                f'  coinset_msp_base_url: "{msp_base_url}"',
                "vault:",
                '  launcher_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"',
                "  custody_threshold: 1",
                "  recovery_threshold: 1",
                "  recovery_clawback_timelock: 3600",
                "  custody_keys:",
                '    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"',
                "      curve: SECP256R1",
                "  recovery_keys:",
                '    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"',
                "      curve: BLS12_381",
            ]
        ),
        encoding="utf-8",
    )
