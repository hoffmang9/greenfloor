"""Coinset HTTP helpers and scanner for vault coin scans."""

from __future__ import annotations

import random
import time
from typing import Any

from greenfloor_scripts.chia_sdk_helpers import (
    coin_id_from_record,
    hex_to_bytes,
    safe_int,
    to_coinset_hex,
)
from greenfloor_scripts.coinset_subprocess import (
    coin_records_cli,
    record_from_cli,
    resolve_client_cli,
)
from greenfloor_scripts.hex_subprocess import normalize_hex_id


def _chunk_values(values: list[str], chunk_size: int) -> list[list[str]]:
    if chunk_size <= 0:
        return [values] if values else []
    return [values[idx : idx + chunk_size] for idx in range(0, len(values), chunk_size)]


def _coin_from_record(*, sdk: Any, record: dict[str, Any]) -> Any | None:
    coin_data = record.get("coin")
    if not isinstance(coin_data, dict):
        return None
    parent_hex = normalize_hex_id(coin_data.get("parent_coin_info"))
    puzzle_hex = normalize_hex_id(coin_data.get("puzzle_hash"))
    if not parent_hex or not puzzle_hex:
        return None
    try:
        return sdk.Coin(
            hex_to_bytes(parent_hex),
            hex_to_bytes(puzzle_hex),
            int(coin_data.get("amount", 0)),
        )
    except Exception:
        return None


def _is_retryable_coinset_error(exc: Exception) -> bool:
    message = str(exc).strip().lower()
    if not message:
        return False
    retry_markers = (
        "coinset_network_error",
        "timed out",
        "timeout",
        "connection reset",
        "connection refused",
        "remote end closed connection",
        "temporary failure",
        "temporarily unavailable",
        "bad gateway",
        "service unavailable",
        "too many requests",
        "http error 429",
        "coinset_http_error:429",
        "coinset_http_error:502",
        "coinset_http_error:503",
        "coinset_http_error:504",
        "ssl",
        "handshake",
        "cloudflare",
    )
    return any(marker in message for marker in retry_markers)


def coinset_with_retries(
    func: Any,
    *,
    attempts: int = 4,
    initial_delay_seconds: float = 0.8,
    jitter_ratio: float = 0.25,
) -> Any:
    delay = max(0.1, float(initial_delay_seconds))
    jitter = min(max(0.0, float(jitter_ratio)), 0.9)
    last_exc: Exception | None = None
    for attempt in range(1, max(1, int(attempts)) + 1):
        try:
            return func()
        except Exception as exc:  # noqa: BLE001
            last_exc = exc
            if attempt >= attempts or not _is_retryable_coinset_error(exc):
                raise
            sleep_multiplier = 1.0 + random.uniform(-jitter, jitter)
            time.sleep(max(0.05, delay * sleep_multiplier))
            delay = min(delay * 2.0, 8.0)
    if last_exc is not None:
        raise last_exc
    raise RuntimeError("coinset_retry_logic_unreachable")


class CoinsetScanner:
    def __init__(self, *, network: str, base_url: str | None = None) -> None:
        self.network, self.base_url = resolve_client_cli(network, base_url)

    def _records(
        self,
        endpoint: str,
        body: dict[str, Any],
        *,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return coinset_with_retries(
            lambda: coin_records_cli(
                self.network,
                self.base_url,
                endpoint,
                body,
                start_height=start_height,
                end_height=end_height,
            )
        )

    def _record(
        self,
        endpoint: str,
        body: dict[str, Any],
        key: str,
    ) -> dict[str, Any] | None:
        return coinset_with_retries(
            lambda: record_from_cli(
                self.network,
                self.base_url,
                endpoint,
                body,
                key,
            )
        )

    def get_blockchain_state(self) -> dict[str, Any] | None:
        return self._record("get_blockchain_state", {}, "blockchain_state")

    def by_puzzle_hash(
        self,
        *,
        puzzle_hash: str,
        include_spent: bool,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return self._records(
            "get_coin_records_by_puzzle_hash",
            {
                "puzzle_hash": puzzle_hash,
                "include_spent_coins": include_spent,
            },
            start_height=start_height,
            end_height=end_height,
        )

    def by_puzzle_hashes(
        self,
        *,
        puzzle_hashes: list[str],
        include_spent: bool,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        if not puzzle_hashes:
            return []
        return self._records(
            "get_coin_records_by_puzzle_hashes",
            {
                "puzzle_hashes": puzzle_hashes,
                "include_spent_coins": include_spent,
            },
            start_height=start_height,
            end_height=end_height,
        )

    def by_hint(
        self,
        *,
        hint: str,
        include_spent: bool,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return self._records(
            "get_coin_records_by_hint",
            {
                "hint": hint,
                "include_spent_coins": include_spent,
            },
            start_height=start_height,
            end_height=end_height,
        )

    def by_hints(
        self,
        *,
        hints: list[str],
        include_spent: bool,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        if not hints:
            return []
        return self._records(
            "get_coin_records_by_hints",
            {
                "hints": hints,
                "include_spent_coins": include_spent,
            },
            start_height=start_height,
            end_height=end_height,
        )

    def by_names(
        self,
        *,
        coin_names: list[str],
        include_spent: bool = True,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        if not coin_names:
            return []
        return self._records(
            "get_coin_records_by_names",
            {
                "names": coin_names,
                "include_spent_coins": include_spent,
            },
            start_height=start_height,
            end_height=end_height,
        )

    def puzzle_and_solution(
        self, *, coin_id_hex: str, height: int | None = None
    ) -> dict[str, Any] | None:
        body: dict[str, Any] = {"coin_id": coin_id_hex}
        if height is not None and height > 0:
            body["height"] = int(height)
        return self._record("get_puzzle_and_solution", body, "coin_solution")

    def existing_coin_names(self, *, coin_ids_hex: list[str]) -> set[str]:
        """Return the subset of coin ids that Coinset resolves by exact name."""
        existing: set[str] = set()
        if not coin_ids_hex:
            return existing
        for batch in _chunk_values(coin_ids_hex, 200):
            rows = self.by_names(
                coin_names=[to_coinset_hex(hex_to_bytes(coin_id)) for coin_id in batch],
                include_spent=True,
            )
            for record in rows:
                resolved = coin_id_from_record(record)
                if resolved:
                    existing.add(resolved)
        return existing


def _detect_cat_asset_id(
    *,
    sdk: Any,
    coinset: CoinsetScanner,
    coin_id: str,
    record: dict[str, Any],
    cat_asset_cache: dict[str, str],
    parent_record_cache: dict[str, dict[str, Any] | None],
    puzzle_solution_cache: dict[str, dict[str, Any] | None],
    parent_lineage_cache: dict[str, dict[str, Any]],
) -> str | None:
    cached = cat_asset_cache.get(coin_id)
    if cached is not None:
        return cached or None
    coin = _coin_from_record(sdk=sdk, record=record)
    if coin is None:
        cat_asset_cache[coin_id] = ""
        return None
    parent_coin_id_hex = normalize_hex_id(coin.parent_coin_info.hex()) or ""
    if not parent_coin_id_hex:
        cat_asset_cache[coin_id] = ""
        return None
    parent_lineage = parent_lineage_cache.get(parent_coin_id_hex)
    if isinstance(parent_lineage, dict):
        cached_child_assets = parent_lineage.get("child_asset_ids")
        if isinstance(cached_child_assets, dict):
            cached_asset = normalize_hex_id(cached_child_assets.get(coin_id))
            if cached_asset:
                cat_asset_cache[coin_id] = cached_asset
                return cached_asset
            if coin_id in cached_child_assets:
                cat_asset_cache[coin_id] = ""
                return None

    parent_record = parent_record_cache.get(parent_coin_id_hex)
    if parent_record is None and parent_coin_id_hex not in parent_record_cache:
        rows = coinset.by_names(
            coin_names=[to_coinset_hex(coin.parent_coin_info)],
            include_spent=True,
        )
        parent_record = rows[0] if rows else None
        parent_record_cache[parent_coin_id_hex] = parent_record
    if not isinstance(parent_record, dict):
        cat_asset_cache[coin_id] = ""
        return None
    parent_coin = _coin_from_record(sdk=sdk, record=parent_record)
    if parent_coin is None:
        cat_asset_cache[coin_id] = ""
        return None
    spent_height = safe_int(parent_record.get("spent_block_index"), default=0)
    if spent_height <= 0:
        cat_asset_cache[coin_id] = ""
        return None

    parent_coin_name = normalize_hex_id(sdk.to_hex(parent_coin.coin_id())) or ""
    if not parent_coin_name:
        cat_asset_cache[coin_id] = ""
        return None
    solution_cache_key = f"{parent_coin_name}:{spent_height}"
    solution = puzzle_solution_cache.get(solution_cache_key)
    if solution is None and solution_cache_key not in puzzle_solution_cache:
        solution = coinset.puzzle_and_solution(
            coin_id_hex=to_coinset_hex(parent_coin.coin_id()),
            height=spent_height,
        )
        puzzle_solution_cache[solution_cache_key] = solution
    if not isinstance(solution, dict):
        cat_asset_cache[coin_id] = ""
        return None
    puzzle_reveal_hex = str(solution.get("puzzle_reveal", "")).strip()
    solution_hex = str(solution.get("solution", "")).strip()
    if not puzzle_reveal_hex or not solution_hex:
        cat_asset_cache[coin_id] = ""
        return None
    try:
        clvm = sdk.Clvm()
        parent_puzzle_program = clvm.deserialize(hex_to_bytes(puzzle_reveal_hex))
        parent_solution_program = clvm.deserialize(hex_to_bytes(solution_hex))
        parsed_children = parent_puzzle_program.puzzle().parse_child_cats(
            parent_coin, parent_solution_program
        )
    except Exception:
        cat_asset_cache[coin_id] = ""
        return None
    if not parsed_children:
        parent_lineage_cache[parent_coin_id_hex] = {
            "spent_height": spent_height,
            "child_asset_ids": {coin_id: ""},
        }
        cat_asset_cache[coin_id] = ""
        return None
    wanted_id = sdk.to_hex(coin.coin_id())
    child_assets: dict[str, str] = {}
    for cat in parsed_children:
        child_coin = getattr(cat, "coin", None)
        info = getattr(cat, "info", None)
        if child_coin is None or info is None:
            continue
        child_id = normalize_hex_id(sdk.to_hex(child_coin.coin_id())) or ""
        if not child_id:
            continue
        asset_id = normalize_hex_id(sdk.to_hex(info.asset_id)) or ""
        child_assets[child_id] = asset_id
        cat_asset_cache[child_id] = asset_id

    if coin_id not in child_assets:
        child_assets[coin_id] = ""
    parent_lineage_cache[parent_coin_id_hex] = {
        "spent_height": spent_height,
        "child_asset_ids": child_assets,
    }
    target_asset = child_assets.get(wanted_id) or child_assets.get(coin_id) or ""
    if target_asset:
        cat_asset_cache[coin_id] = target_asset
        return target_asset
    cat_asset_cache[coin_id] = ""
    return None
