"""Coinset read/mutation client for Python scripts (CLI subprocess backend)."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.coinset.cli import post_json_cli, push_tx_cli


def _normalize_coinset_network(network: str) -> str:
    normalized = network.strip().lower()
    if normalized in {"testnet", "testnet11"}:
        return "testnet11"
    if normalized == "mainnet":
        return "mainnet"
    return "mainnet"


def _apply_height_range(
    body: dict[str, Any],
    start_height: int | None,
    end_height: int | None,
) -> None:
    if start_height is not None:
        body["start_height"] = int(start_height)
    if end_height is not None:
        body["end_height"] = int(end_height)


class CoinsetReadClient:
    MAINNET_BASE_URL = "https://api.coinset.org"
    TESTNET11_BASE_URL = "https://testnet11.api.coinset.org"

    def __init__(
        self,
        base_url: str | None = None,
        *,
        network: str = "mainnet",
    ) -> None:
        self.network = _normalize_coinset_network(network)
        resolved_base_url = base_url.strip() if isinstance(base_url, str) else ""
        if not resolved_base_url:
            if self.network == "testnet11":
                resolved_base_url = self.TESTNET11_BASE_URL
            else:
                resolved_base_url = self.MAINNET_BASE_URL
        self.base_url = resolved_base_url.rstrip("/")

    def post_json(self, endpoint: str, body: dict[str, Any]) -> dict[str, Any]:
        payload = post_json_cli(self.network, self.base_url, endpoint, body)
        if not isinstance(payload, dict):
            raise RuntimeError("coinset_invalid_response_payload")
        return payload

    def _records_from_post(self, endpoint: str, body: dict[str, Any]) -> list[dict[str, Any]]:
        payload = self.post_json(endpoint, body)
        if not payload.get("success", False):
            return []
        records = payload.get("coin_records") or []
        if not isinstance(records, list):
            return []
        return [record for record in records if isinstance(record, dict)]

    def _record_from_post(
        self,
        endpoint: str,
        body: dict[str, Any],
        key: str,
    ) -> dict[str, Any] | None:
        payload = self.post_json(endpoint, body)
        if not payload.get("success", False):
            return None
        record = payload.get(key)
        if not isinstance(record, dict):
            return None
        return record

    def _coin_records_query(
        self,
        endpoint: str,
        body: dict[str, Any],
        *,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        _apply_height_range(body, start_height, end_height)
        return self._records_from_post(endpoint, body)

    def _coin_records_list_query(
        self,
        endpoint: str,
        *,
        list_field: str,
        values_hex: list[str],
        include_spent_coins: bool,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        if not values_hex:
            return []
        body: dict[str, Any] = {
            list_field: [str(value) for value in values_hex],
            "include_spent_coins": bool(include_spent_coins),
        }
        return self._coin_records_query(
            endpoint,
            body,
            start_height=start_height,
            end_height=end_height,
        )

    def get_all_mempool_tx_ids(self) -> list[str]:
        payload = self.post_json("get_all_mempool_tx_ids", {})
        if not payload.get("success", False):
            return []
        tx_ids = payload.get("tx_ids") or payload.get("mempool_tx_ids") or []
        return [str(value) for value in tx_ids]

    def get_coin_records_by_puzzle_hash(
        self,
        *,
        puzzle_hash_hex: str,
        include_spent_coins: bool = False,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return self._coin_records_query(
            "get_coin_records_by_puzzle_hash",
            {
                "puzzle_hash": puzzle_hash_hex,
                "include_spent_coins": include_spent_coins,
            },
            start_height=start_height,
            end_height=end_height,
        )

    def get_coin_records_by_puzzle_hashes(
        self,
        *,
        puzzle_hashes_hex: list[str],
        include_spent_coins: bool = False,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return self._coin_records_list_query(
            "get_coin_records_by_puzzle_hashes",
            list_field="puzzle_hashes",
            values_hex=puzzle_hashes_hex,
            include_spent_coins=include_spent_coins,
            start_height=start_height,
            end_height=end_height,
        )

    def get_coin_record_by_name(self, *, coin_name_hex: str) -> dict[str, Any] | None:
        return self._record_from_post(
            "get_coin_record_by_name",
            {"name": coin_name_hex},
            "coin_record",
        )

    def get_coin_records_by_names(
        self,
        *,
        coin_names_hex: list[str],
        include_spent_coins: bool = True,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return self._coin_records_list_query(
            "get_coin_records_by_names",
            list_field="names",
            values_hex=coin_names_hex,
            include_spent_coins=include_spent_coins,
            start_height=start_height,
            end_height=end_height,
        )

    def get_coin_records_by_parent_ids(
        self,
        *,
        parent_ids_hex: list[str],
        include_spent_coins: bool = True,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return self._coin_records_list_query(
            "get_coin_records_by_parent_ids",
            list_field="parent_ids",
            values_hex=parent_ids_hex,
            include_spent_coins=include_spent_coins,
            start_height=start_height,
            end_height=end_height,
        )

    def get_coin_records_by_hint(
        self,
        *,
        hint_hex: str,
        include_spent_coins: bool = False,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return self._coin_records_query(
            "get_coin_records_by_hint",
            {
                "hint": hint_hex,
                "include_spent_coins": bool(include_spent_coins),
            },
            start_height=start_height,
            end_height=end_height,
        )

    def get_coin_records_by_hints(
        self,
        *,
        hints_hex: list[str],
        include_spent_coins: bool = False,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return self._coin_records_list_query(
            "get_coin_records_by_hints",
            list_field="hints",
            values_hex=hints_hex,
            include_spent_coins=include_spent_coins,
            start_height=start_height,
            end_height=end_height,
        )

    def get_puzzle_and_solution(
        self,
        *,
        coin_id_hex: str,
        height: int | None = None,
    ) -> dict[str, Any] | None:
        body: dict[str, Any] = {"coin_id": coin_id_hex}
        if height is not None and height > 0:
            body["height"] = int(height)
        return self._record_from_post("get_puzzle_and_solution", body, "coin_solution")

    def get_blockchain_state(self) -> dict[str, Any] | None:
        payload = self.post_json("get_blockchain_state", {})
        if not payload.get("success", False):
            return None
        blockchain_state = payload.get("blockchain_state")
        if isinstance(blockchain_state, dict):
            return blockchain_state
        return payload


class CoinsetAdapter(CoinsetReadClient):
    def push_tx(self, *, spend_bundle_hex: str) -> dict[str, object]:
        payload = push_tx_cli(self.network, self.base_url, spend_bundle_hex)
        if not isinstance(payload, dict):
            raise RuntimeError("coinset_push_tx_invalid_response")
        return payload


def build_webhook_callback_url(listen_addr: str, path: str = "/coinset/tx-block") -> str:
    host, _, port = listen_addr.partition(":")
    if not port:
        port = "8787"
    return f"http://{host}:{port}{path}"
