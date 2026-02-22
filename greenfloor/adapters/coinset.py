from __future__ import annotations

import json
import urllib.parse
import urllib.request
from typing import Any


class CoinsetAdapter:
    MAINNET_BASE_URL = "https://api.coinset.org"
    TESTNET11_BASE_URL = "https://api-testnet11.coinset.org"

    def __init__(
        self,
        base_url: str | None = None,
        *,
        network: str = "mainnet",
        require_testnet11: bool = False,
    ) -> None:
        selected_network = "testnet11" if require_testnet11 else network.strip().lower()
        if selected_network not in {"mainnet", "testnet11"}:
            selected_network = "mainnet"
        resolved_base_url = base_url.strip() if isinstance(base_url, str) else ""
        if not resolved_base_url:
            if selected_network == "testnet11":
                resolved_base_url = self.TESTNET11_BASE_URL
            else:
                resolved_base_url = self.MAINNET_BASE_URL
        self.base_url = resolved_base_url.rstrip("/")

    def _post_json(self, endpoint: str, body: dict[str, Any]) -> dict[str, Any]:
        url = f"{self.base_url}/{endpoint.lstrip('/')}"
        data = json.dumps(body).encode("utf-8")
        req = urllib.request.Request(
            url,
            data=data,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(req, timeout=15) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
        if isinstance(payload, dict):
            return payload
        return {}

    def get_all_mempool_tx_ids(self) -> list[str]:
        payload = self._post_json("get_all_mempool_tx_ids", {})
        if not payload.get("success", False):
            return []
        # Field naming may vary; keep robust fallback.
        tx_ids = payload.get("tx_ids") or payload.get("mempool_tx_ids") or []
        return [str(x) for x in tx_ids]

    def get_coin_records_by_puzzle_hash(
        self,
        *,
        puzzle_hash_hex: str,
        include_spent_coins: bool = False,
    ) -> list[dict[str, Any]]:
        payload = self._post_json(
            "get_coin_records_by_puzzle_hash",
            {
                "puzzle_hash": puzzle_hash_hex,
                "include_spent_coins": include_spent_coins,
            },
        )
        if not payload.get("success", False):
            return []
        records = payload.get("coin_records") or []
        if not isinstance(records, list):
            return []
        return [r for r in records if isinstance(r, dict)]

    def get_coin_record_by_name(self, *, coin_name_hex: str) -> dict[str, Any] | None:
        payload = self._post_json("get_coin_record_by_name", {"name": coin_name_hex})
        if not payload.get("success", False):
            return None
        record = payload.get("coin_record")
        if not isinstance(record, dict):
            return None
        return record

    def get_puzzle_and_solution(
        self,
        *,
        coin_id_hex: str,
        height: int | None = None,
    ) -> dict[str, Any] | None:
        body: dict[str, Any] = {"coin_id": coin_id_hex}
        if height is not None and height > 0:
            body["height"] = int(height)
        payload = self._post_json("get_puzzle_and_solution", body)
        if not payload.get("success", False):
            return None
        coin_solution = payload.get("coin_solution")
        if not isinstance(coin_solution, dict):
            return None
        return coin_solution

    def push_tx(self, *, spend_bundle_hex: str) -> dict[str, Any]:
        payload = self._post_json("push_tx", {"spend_bundle": spend_bundle_hex})
        if not isinstance(payload, dict):
            return {"success": False, "error": "invalid_response_payload"}
        return payload


def build_webhook_callback_url(listen_addr: str, path: str = "/coinset/tx-block") -> str:
    host, _, port = listen_addr.partition(":")
    if not port:
        port = "8787"
    # Use http callback by default; production deployments can terminate TLS elsewhere.
    return f"http://{host}:{port}{path}"
