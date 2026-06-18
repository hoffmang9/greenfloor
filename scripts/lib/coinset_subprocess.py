"""Subprocess bridge to ``greenfloor-engine coinset`` adapter CLI for Python scripts."""

from __future__ import annotations

import json
import subprocess
from typing import Any

from lib.binaries import GreenfloorEngineBinaryError, resolve_greenfloor_engine_binary

MAINNET_BASE_URL = "https://api.coinset.org"
TESTNET11_BASE_URL = "https://testnet11.api.coinset.org"


def _normalize_coinset_network(network: str) -> str:
    normalized = network.strip().lower()
    if normalized in {"testnet", "testnet11"}:
        return "testnet11"
    if normalized == "mainnet":
        return "mainnet"
    return "mainnet"


def _resolve_base_url(base_url: str | None, *, network: str) -> str:
    resolved = base_url.strip() if isinstance(base_url, str) else ""
    if not resolved:
        if _normalize_coinset_network(network) == "testnet11":
            return TESTNET11_BASE_URL
        return MAINNET_BASE_URL
    return resolved.rstrip("/")


def run_engine_json(argv: list[str]) -> Any:
    try:
        binary = resolve_greenfloor_engine_binary(build_if_missing=False)
    except GreenfloorEngineBinaryError as exc:
        raise RuntimeError(f"coinset_cli_binary_unavailable: {exc}") from exc
    cmd = [str(binary), *argv, "--json"]
    result = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "").strip()
        raise RuntimeError(f"coinset_cli_failed:{detail}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError("coinset_cli_invalid_json") from exc


def _client_flags(network: str, base_url: str) -> list[str]:
    return ["--network", network, "--base-url", base_url]


def _height_flags(
    start_height: int | None,
    end_height: int | None,
) -> list[str]:
    flags: list[str] = []
    if start_height is not None:
        flags.extend(["--start-height", str(int(start_height))])
    if end_height is not None:
        flags.extend(["--end-height", str(int(end_height))])
    return flags


def post_json_cli(
    network: str,
    base_url: str,
    endpoint: str,
    body: dict[str, Any],
) -> Any:
    argv = [
        "coinset",
        "post",
        *_client_flags(network, base_url),
        "--endpoint",
        endpoint,
        "--body-json",
        json.dumps(body, separators=(",", ":")),
    ]
    return run_engine_json(argv)


def push_tx_cli(network: str, base_url: str, spend_bundle_hex: str) -> Any:
    argv = [
        "coinset",
        "push-tx",
        *_client_flags(network, base_url),
        "--spend-bundle-hex",
        spend_bundle_hex,
    ]
    return run_engine_json(argv)


def _adapter_json(
    network: str,
    base_url: str,
    subcommand: str,
    flags: list[str],
) -> dict[str, Any]:
    argv = ["coinset", "adapter", subcommand, *_client_flags(network, base_url), *flags]
    payload = run_engine_json(argv)
    if not isinstance(payload, dict):
        raise RuntimeError("coinset_adapter_invalid_response")
    return payload


class CoinsetScriptClient:
    """Script-facing Coinset client backed by native ``greenfloor-engine coinset adapter`` CLI."""

    def __init__(
        self,
        base_url: str | None = None,
        *,
        network: str = "mainnet",
    ) -> None:
        self.network = _normalize_coinset_network(network)
        self.base_url = _resolve_base_url(base_url, network=self.network)

    def post_json(self, endpoint: str, body: dict[str, Any]) -> dict[str, Any]:
        payload = post_json_cli(self.network, self.base_url, endpoint, body)
        if not isinstance(payload, dict):
            raise RuntimeError("coinset_invalid_response_payload")
        return payload

    def get_all_mempool_tx_ids(self) -> list[str]:
        payload = _adapter_json(self.network, self.base_url, "get-mempool-tx-ids", [])
        tx_ids = payload.get("tx_ids") or []
        return [str(value) for value in tx_ids]

    def get_coin_records_by_puzzle_hash(
        self,
        *,
        puzzle_hash_hex: str,
        include_spent_coins: bool = False,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        flags = [
            "--puzzle-hash-hex",
            puzzle_hash_hex,
            "--include-spent-coins",
            str(include_spent_coins).lower(),
            *_height_flags(start_height, end_height),
        ]
        payload = _adapter_json(
            self.network, self.base_url, "get-coin-records-by-puzzle-hash", flags
        )
        records = payload.get("coin_records") or []
        return [record for record in records if isinstance(record, dict)]

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
        flags = [
            "--values-hex",
            ",".join(puzzle_hashes_hex),
            "--include-spent-coins",
            str(include_spent_coins).lower(),
            *_height_flags(start_height, end_height),
        ]
        payload = _adapter_json(
            self.network, self.base_url, "get-coin-records-by-puzzle-hashes", flags
        )
        records = payload.get("coin_records") or []
        return [record for record in records if isinstance(record, dict)]

    def get_coin_record_by_name(self, *, coin_name_hex: str) -> dict[str, Any] | None:
        payload = _adapter_json(
            self.network,
            self.base_url,
            "get-coin-record-by-name",
            ["--coin-name-hex", coin_name_hex],
        )
        record = payload.get("coin_record")
        return record if isinstance(record, dict) else None

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
        flags = [
            "--values-hex",
            ",".join(coin_names_hex),
            "--include-spent-coins",
            str(include_spent_coins).lower(),
            *_height_flags(start_height, end_height),
        ]
        payload = _adapter_json(self.network, self.base_url, "get-coin-records-by-names", flags)
        records = payload.get("coin_records") or []
        return [record for record in records if isinstance(record, dict)]

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
        flags = [
            "--values-hex",
            ",".join(parent_ids_hex),
            "--include-spent-coins",
            str(include_spent_coins).lower(),
            *_height_flags(start_height, end_height),
        ]
        payload = _adapter_json(
            self.network, self.base_url, "get-coin-records-by-parent-ids", flags
        )
        records = payload.get("coin_records") or []
        return [record for record in records if isinstance(record, dict)]

    def get_coin_records_by_hint(
        self,
        *,
        hint_hex: str,
        include_spent_coins: bool = False,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        flags = [
            "--hint-hex",
            hint_hex,
            "--include-spent-coins",
            str(include_spent_coins).lower(),
            *_height_flags(start_height, end_height),
        ]
        payload = _adapter_json(self.network, self.base_url, "get-coin-records-by-hint", flags)
        records = payload.get("coin_records") or []
        return [record for record in records if isinstance(record, dict)]

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
        flags = [
            "--values-hex",
            ",".join(hints_hex),
            "--include-spent-coins",
            str(include_spent_coins).lower(),
            *_height_flags(start_height, end_height),
        ]
        payload = _adapter_json(self.network, self.base_url, "get-coin-records-by-hints", flags)
        records = payload.get("coin_records") or []
        return [record for record in records if isinstance(record, dict)]

    def get_puzzle_and_solution(
        self,
        *,
        coin_id_hex: str,
        height: int | None = None,
    ) -> dict[str, Any] | None:
        flags = ["--coin-id-hex", coin_id_hex]
        if height is not None and height > 0:
            flags.extend(["--height", str(int(height))])
        payload = _adapter_json(self.network, self.base_url, "get-puzzle-and-solution", flags)
        solution = payload.get("coin_solution")
        return solution if isinstance(solution, dict) else None

    def get_blockchain_state(self) -> dict[str, Any] | None:
        payload = _adapter_json(self.network, self.base_url, "get-blockchain-state", [])
        state = payload.get("blockchain_state")
        return state if isinstance(state, dict) else None

    def push_tx(self, *, spend_bundle_hex: str) -> dict[str, object]:
        payload = push_tx_cli(self.network, self.base_url, spend_bundle_hex)
        if not isinstance(payload, dict):
            raise RuntimeError("coinset_push_tx_invalid_response")
        return payload
