from __future__ import annotations

import json
import urllib.parse
import urllib.request


class CoinsetAdapter:
    def __init__(self, base_url: str) -> None:
        self.base_url = base_url.rstrip("/")

    def get_all_mempool_tx_ids(self) -> list[str]:
        url = f"{self.base_url}/get_all_mempool_tx_ids"
        data = json.dumps({}).encode("utf-8")
        req = urllib.request.Request(
            url,
            data=data,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(req, timeout=15) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
        if not payload.get("success", False):
            return []
        # Field naming may vary; keep robust fallback.
        tx_ids = payload.get("tx_ids") or payload.get("mempool_tx_ids") or []
        return [str(x) for x in tx_ids]


def build_webhook_callback_url(listen_addr: str, path: str = "/coinset/tx-block") -> str:
    host, _, port = listen_addr.partition(":")
    if not port:
        port = "8787"
    # Use http callback by default; production deployments can terminate TLS elsewhere.
    return f"http://{host}:{port}{path}"
