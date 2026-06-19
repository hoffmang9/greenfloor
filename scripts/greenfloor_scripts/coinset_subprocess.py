"""Subprocess bridge to ``greenfloor-engine coinset`` for Python scripts."""

from __future__ import annotations

import json
from typing import Any

from greenfloor_scripts.engine_subprocess import run_engine_json


def _client_flags(network: str, base_url: str | None) -> list[str]:
    flags = ["--network", network]
    if base_url:
        flags.extend(["--base-url", base_url])
    return flags


def _height_fields(
    body: dict[str, Any],
    *,
    start_height: int | None,
    end_height: int | None,
) -> None:
    if start_height is not None:
        body["start_height"] = int(start_height)
    if end_height is not None:
        body["end_height"] = int(end_height)


def _coin_records(payload: dict[str, Any]) -> list[dict[str, Any]]:
    if not payload.get("success"):
        return []
    records = payload.get("coin_records") or []
    return [record for record in records if isinstance(record, dict)]


def _record(payload: dict[str, Any], key: str) -> dict[str, Any] | None:
    if not payload.get("success"):
        return None
    record = payload.get(key)
    return record if isinstance(record, dict) else None


def post_json_cli(
    network: str,
    base_url: str | None,
    endpoint: str,
    body: dict[str, Any],
) -> dict[str, Any]:
    argv = [
        "coinset",
        "post",
        *_client_flags(network, base_url),
        "--endpoint",
        endpoint,
        "--body-json",
        json.dumps(body, separators=(",", ":")),
    ]
    payload = run_engine_json(argv)
    if not isinstance(payload, dict):
        raise RuntimeError("coinset_invalid_response_payload")
    return payload


def push_tx_cli(network: str, base_url: str | None, spend_bundle_hex: str) -> dict[str, Any]:
    argv = [
        "coinset",
        "push-tx",
        *_client_flags(network, base_url),
        "--spend-bundle-hex",
        spend_bundle_hex,
    ]
    payload = run_engine_json(argv)
    if not isinstance(payload, dict):
        raise RuntimeError("coinset_push_tx_invalid_response")
    return payload


class CoinsetScriptClient:
    """Script-facing Coinset client backed by ``greenfloor-engine coinset post``."""

    def __init__(
        self,
        base_url: str | None = None,
        *,
        network: str = "mainnet",
    ) -> None:
        self.network = network.strip()
        self.base_url = base_url.strip() if isinstance(base_url, str) and base_url.strip() else None

    def post_json(self, endpoint: str, body: dict[str, Any]) -> dict[str, Any]:
        return post_json_cli(self.network, self.base_url, endpoint, body)

    def get_all_mempool_tx_ids(self) -> list[str]:
        payload = self.post_json("get_all_mempool_tx_ids", {})
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
        body: dict[str, Any] = {
            "puzzle_hash": puzzle_hash_hex,
            "include_spent_coins": include_spent_coins,
        }
        _height_fields(body, start_height=start_height, end_height=end_height)
        return _coin_records(self.post_json("get_coin_records_by_puzzle_hash", body))

    def get_coin_records_by_puzzle_hashes(
        self,
        *,
        puzzle_hashes_hex: list[str],
        include_spent_coins: bool = False,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        if not puzzle_hashes_hex:
            return []
        body: dict[str, Any] = {
            "puzzle_hashes": puzzle_hashes_hex,
            "include_spent_coins": include_spent_coins,
        }
        _height_fields(body, start_height=start_height, end_height=end_height)
        return _coin_records(self.post_json("get_coin_records_by_puzzle_hashes", body))

    def get_coin_record_by_name(self, *, coin_name_hex: str) -> dict[str, Any] | None:
        return _record(
            self.post_json("get_coin_record_by_name", {"name": coin_name_hex}),
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
        if not coin_names_hex:
            return []
        body: dict[str, Any] = {
            "names": coin_names_hex,
            "include_spent_coins": include_spent_coins,
        }
        _height_fields(body, start_height=start_height, end_height=end_height)
        return _coin_records(self.post_json("get_coin_records_by_names", body))

    def get_coin_records_by_parent_ids(
        self,
        *,
        parent_ids_hex: list[str],
        include_spent_coins: bool = True,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        if not parent_ids_hex:
            return []
        body: dict[str, Any] = {
            "parent_ids": parent_ids_hex,
            "include_spent_coins": include_spent_coins,
        }
        _height_fields(body, start_height=start_height, end_height=end_height)
        return _coin_records(self.post_json("get_coin_records_by_parent_ids", body))

    def get_coin_records_by_hint(
        self,
        *,
        hint_hex: str,
        include_spent_coins: bool = False,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        body: dict[str, Any] = {
            "hint": hint_hex,
            "include_spent_coins": include_spent_coins,
        }
        _height_fields(body, start_height=start_height, end_height=end_height)
        return _coin_records(self.post_json("get_coin_records_by_hint", body))

    def get_coin_records_by_hints(
        self,
        *,
        hints_hex: list[str],
        include_spent_coins: bool = False,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        if not hints_hex:
            return []
        body: dict[str, Any] = {
            "hints": hints_hex,
            "include_spent_coins": include_spent_coins,
        }
        _height_fields(body, start_height=start_height, end_height=end_height)
        return _coin_records(self.post_json("get_coin_records_by_hints", body))

    def get_puzzle_and_solution(
        self,
        *,
        coin_id_hex: str,
        height: int | None = None,
    ) -> dict[str, Any] | None:
        body: dict[str, Any] = {"coin_id": coin_id_hex}
        if height is not None and height > 0:
            body["height"] = int(height)
        return _record(self.post_json("get_puzzle_and_solution", body), "coin_solution")

    def get_blockchain_state(self) -> dict[str, Any] | None:
        return _record(self.post_json("get_blockchain_state", {}), "blockchain_state")

    def push_tx(self, *, spend_bundle_hex: str) -> dict[str, object]:
        return push_tx_cli(self.network, self.base_url, spend_bundle_hex)
