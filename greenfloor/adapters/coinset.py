from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any

_COINSET_TX_ID_KEYS = (
    "tx_id",
    "txId",
    "take_tx_id",
    "takeTxId",
    "settlement_tx_id",
    "settlementTxId",
    "coinset_tx_id",
    "coinsetTxId",
    "block_tx_id",
    "blockTxId",
    "mempool_tx_ids",
    "mempoolTxIds",
    "confirmed_tx_ids",
    "confirmedTxIds",
)


def _looks_like_tx_id(value: object) -> bool:
    if not isinstance(value, str):
        return False
    normalized = value.strip().lower()
    return len(normalized) == 64 and all(ch in "0123456789abcdef" for ch in normalized)


def extract_coinset_tx_ids_from_offer_payload(payload: dict[str, Any]) -> list[str]:
    tx_ids: list[str] = []

    def _add_candidate(candidate: object) -> None:
        if isinstance(candidate, str):
            normalized = candidate.strip().lower()
            if _looks_like_tx_id(normalized) and normalized not in tx_ids:
                tx_ids.append(normalized)
        elif isinstance(candidate, list):
            for item in candidate:
                _add_candidate(item)

    def _walk(node: object) -> None:
        if isinstance(node, dict):
            for key, value in node.items():
                if key in _COINSET_TX_ID_KEYS:
                    _add_candidate(value)
                # Some providers nest tx metadata under "offer"/"data"/etc.
                if isinstance(value, dict | list):
                    _walk(value)
            return
        if isinstance(node, list):
            for item in node:
                if isinstance(item, dict | list):
                    _walk(item)

    _walk(payload)
    return tx_ids


class CoinsetAdapter:
    MAINNET_BASE_URL = "https://api.coinset.org"
    TESTNET11_BASE_URL = "https://testnet11.api.coinset.org"

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
        self.network = selected_network
        resolved_base_url = base_url.strip() if isinstance(base_url, str) else ""
        if not resolved_base_url:
            if selected_network == "testnet11":
                resolved_base_url = self.TESTNET11_BASE_URL
            else:
                resolved_base_url = self.MAINNET_BASE_URL
        self.base_url = resolved_base_url.rstrip("/")

    def _post_json_once(
        self, endpoint: str, body: dict[str, Any], *, base_url: str
    ) -> dict[str, Any]:
        url = f"{base_url.rstrip('/')}/{endpoint.lstrip('/')}"
        data = json.dumps(body).encode("utf-8")
        req = urllib.request.Request(
            url,
            data=data,
            method="POST",
            headers={
                "Content-Type": "application/json",
                "Accept": "application/json",
                "User-Agent": "greenfloor/0.1 (+https://github.com/hoffmang/greenfloor)",
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=15) as resp:
                payload = json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            raw = exc.read().decode("utf-8", errors="replace").strip()
            snippet = raw[:160] if raw else ""
            message = f"coinset_http_error:{exc.code}"
            if snippet:
                message = f"{message}:{snippet}"
            raise RuntimeError(message) from exc
        except urllib.error.URLError as exc:
            raise RuntimeError(f"coinset_network_error:{exc.reason}") from exc
        if isinstance(payload, dict):
            return payload
        raise RuntimeError("coinset_invalid_response_payload")

    def _post_json(self, endpoint: str, body: dict[str, Any]) -> dict[str, Any]:
        request_body = dict(body)
        if self.network == "testnet11":
            # Force testnet selection for shared/multiplexed Coinset backends.
            request_body.setdefault("network", "testnet11")
        return self._post_json_once(endpoint, request_body, base_url=self.base_url)

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

    def get_fee_estimate(self, *, target_times: list[int] | None = None) -> dict[str, Any]:
        payload = self._post_json(
            "get_fee_estimate",
            {"target_times": target_times or [60, 300, 600], "cost": 1_000_000},
        )
        if not isinstance(payload, dict):
            return {"success": False, "error": "invalid_response_payload"}
        return payload

    def get_conservative_fee_estimate(self) -> int | None:
        payload = self.get_fee_estimate(target_times=[300, 600, 1200])
        if not payload.get("success", False):
            return None
        estimates = payload.get("estimates")
        if isinstance(estimates, list) and estimates:
            valid = []
            for value in estimates:
                try:
                    parsed = int(value)
                except (TypeError, ValueError):
                    continue
                if parsed >= 0:
                    valid.append(parsed)
            if valid:
                return max(valid)
        fee = payload.get("fee_estimate")
        if fee is None:
            return None
        try:
            parsed_fee = int(fee)
        except (TypeError, ValueError):
            return None
        return parsed_fee if parsed_fee >= 0 else None

    def get_blockchain_state(self) -> dict[str, Any] | None:
        payload = self._post_json("get_blockchain_state", {})
        if not payload.get("success", False):
            return None
        blockchain_state = payload.get("blockchain_state")
        if isinstance(blockchain_state, dict):
            return blockchain_state
        return payload


def build_webhook_callback_url(listen_addr: str, path: str = "/coinset/tx-block") -> str:
    host, _, port = listen_addr.partition(":")
    if not port:
        port = "8787"
    # Use http callback by default; production deployments can terminate TLS elsewhere.
    return f"http://{host}:{port}{path}"
