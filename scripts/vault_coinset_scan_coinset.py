"""Coinset HTTP helpers and scanner for vault coin scans."""

from __future__ import annotations

import importlib
import json
import random
import time
import urllib.error
import urllib.request
from typing import Any

from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.hex_utils import normalize_hex_id


def _import_sdk() -> Any:
    return importlib.import_module("chia_wallet_sdk")


def _hex_to_bytes(value: str) -> bytes:
    raw = value.strip().lower()
    if raw.startswith("0x"):
        raw = raw[2:]
    if len(raw) % 2:
        raw = f"0{raw}"
    return bytes.fromhex(raw)


def _to_coinset_hex(value: bytes) -> str:
    return f"0x{value.hex()}"


def _safe_int(value: object, default: int = 0) -> int:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return default


def _coin_id_from_record(record: dict[str, Any]) -> str:
    coin = record.get("coin")
    if not isinstance(coin, dict):
        return ""
    for candidate in (
        coin.get("name"),
        coin.get("coin_id"),
        coin.get("coin_name"),
        record.get("name"),
    ):
        normalized = normalize_hex_id(candidate)
        if normalized:
            return normalized
    parent_hex = normalize_hex_id(coin.get("parent_coin_info"))
    puzzle_hex = normalize_hex_id(coin.get("puzzle_hash"))
    amount = _safe_int(coin.get("amount"), default=-1)
    # Only synthesize a coin id from canonical fields. Any non-canonical parent
    # or puzzle hash should be treated as invalid row data, not padded/coerced
    # into a potentially fake coin id.
    if not parent_hex or not puzzle_hex or amount < 0 or amount > 0xFFFFFFFFFFFFFFFF:
        return ""
    try:
        sdk = _import_sdk()
        coin = sdk.Coin(_hex_to_bytes(parent_hex), _hex_to_bytes(puzzle_hex), int(amount))
        return normalize_hex_id(sdk.to_hex(coin.coin_id())) or ""
    except Exception:
        return ""


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
            _hex_to_bytes(parent_hex), _hex_to_bytes(puzzle_hex), int(coin_data.get("amount", 0))
        )
    except Exception:
        return None


def _normalize_coinset_base_url(*, base_url: str | None, network: str) -> str | None:
    raw = str(base_url or "").strip()
    if not raw:
        return None
    normalized = raw.rstrip("/")
    lower = normalized.lower()
    mainnet_aliases = {
        "coinset.org",
        "https://coinset.org",
        "http://coinset.org",
        "www.coinset.org",
        "https://www.coinset.org",
        "http://www.coinset.org",
    }
    testnet_aliases = {
        "testnet11.coinset.org",
        "https://testnet11.coinset.org",
        "http://testnet11.coinset.org",
        "www.testnet11.coinset.org",
        "https://www.testnet11.coinset.org",
        "http://www.testnet11.coinset.org",
    }
    is_testnet11 = network.strip().lower() in {"testnet", "testnet11"}
    if lower in mainnet_aliases:
        return (
            CoinsetAdapter.TESTNET11_BASE_URL if is_testnet11 else CoinsetAdapter.MAINNET_BASE_URL
        )
    if lower in testnet_aliases:
        return CoinsetAdapter.TESTNET11_BASE_URL
    return normalized


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


def _coinset_with_retries(
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
        require_testnet11 = network.strip().lower() in {"testnet", "testnet11"}
        resolved_base_url = _normalize_coinset_base_url(base_url=base_url, network=network)
        self.adapter = CoinsetAdapter(
            base_url=resolved_base_url, network=network, require_testnet11=require_testnet11
        )

    def _post_json(self, endpoint: str, body: dict[str, Any]) -> dict[str, Any]:
        def _request_once() -> dict[str, Any]:
            payload = dict(body)
            if self.adapter.network == "testnet11":
                payload.setdefault("network", "testnet11")
            req = urllib.request.Request(
                f"{self.adapter.base_url}/{endpoint}",
                data=json.dumps(payload).encode("utf-8"),
                method="POST",
                headers={
                    "Content-Type": "application/json",
                    "Accept": "application/json",
                    "User-Agent": "greenfloor-vault-coinset-scanner/0.1",
                },
            )
            with urllib.request.urlopen(req, timeout=20) as resp:
                response_payload = json.loads(resp.read().decode("utf-8"))
            if not isinstance(response_payload, dict):
                raise RuntimeError("coinset_invalid_response_payload")
            return response_payload

        parsed = _coinset_with_retries(_request_once)
        if not isinstance(parsed, dict):
            raise RuntimeError("coinset_invalid_response_payload")
        return parsed

    def by_puzzle_hash(
        self,
        *,
        puzzle_hash: str,
        include_spent: bool,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return _coinset_with_retries(
            lambda: self.adapter.get_coin_records_by_puzzle_hash(
                puzzle_hash_hex=puzzle_hash,
                include_spent_coins=include_spent,
                start_height=start_height,
                end_height=end_height,
            )
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
        return _coinset_with_retries(
            lambda: self.adapter.get_coin_records_by_puzzle_hashes(
                puzzle_hashes_hex=puzzle_hashes,
                include_spent_coins=include_spent,
                start_height=start_height,
                end_height=end_height,
            )
        )

    def by_hint(
        self,
        *,
        hint: str,
        include_spent: bool,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        payload = self._post_json(
            "get_coin_records_by_hint",
            {
                "hint": hint,
                "include_spent_coins": include_spent,
                **({"start_height": int(start_height)} if start_height is not None else {}),
                **({"end_height": int(end_height)} if end_height is not None else {}),
            },
        )
        if not payload.get("success", False):
            return []
        rows = payload.get("coin_records") or []
        return [row for row in rows if isinstance(row, dict)]

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
        return _coinset_with_retries(
            lambda: self.adapter.get_coin_records_by_hints(
                hints_hex=hints,
                include_spent_coins=include_spent,
                start_height=start_height,
                end_height=end_height,
            )
        )

    def by_names(
        self, *, coin_names: list[str], include_spent: bool = True
    ) -> list[dict[str, Any]]:
        if not coin_names:
            return []
        return _coinset_with_retries(
            lambda: self.adapter.get_coin_records_by_names(
                coin_names_hex=coin_names,
                include_spent_coins=include_spent,
            )
        )

    def existing_coin_names(self, *, coin_ids_hex: list[str]) -> set[str]:
        """Return the subset of coin ids that Coinset resolves by exact name."""
        existing: set[str] = set()
        if not coin_ids_hex:
            return existing
        for batch in _chunk_values(coin_ids_hex, 200):
            rows = self.by_names(
                coin_names=[_to_coinset_hex(_hex_to_bytes(coin_id)) for coin_id in batch],
                include_spent=True,
            )
            for record in rows:
                coin_id = _coin_id_from_record(record)
                if coin_id:
                    existing.add(coin_id)
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
        child_assets = parent_lineage.get("child_asset_ids")
        if isinstance(child_assets, dict):
            cached_asset = normalize_hex_id(child_assets.get(coin_id))
            if cached_asset:
                cat_asset_cache[coin_id] = cached_asset
                return cached_asset
            # Cached lineage says this child is not a CAT child.
            if coin_id in child_assets:
                cat_asset_cache[coin_id] = ""
                return None

    parent_record = parent_record_cache.get(parent_coin_id_hex)
    if parent_record is None and parent_coin_id_hex not in parent_record_cache:
        parent_record = _coinset_with_retries(
            lambda: coinset.by_names(
                coin_names=[_to_coinset_hex(coin.parent_coin_info)],
                include_spent=True,
            )
        )
        if isinstance(parent_record, list):
            parent_record = parent_record[0] if parent_record else None
        parent_record_cache[parent_coin_id_hex] = parent_record
    if not isinstance(parent_record, dict):
        cat_asset_cache[coin_id] = ""
        return None
    parent_coin = _coin_from_record(sdk=sdk, record=parent_record)
    if parent_coin is None:
        cat_asset_cache[coin_id] = ""
        return None
    spent_height = _safe_int(parent_record.get("spent_block_index"), default=0)
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
        solution = _coinset_with_retries(
            lambda: coinset.adapter.get_puzzle_and_solution(
                coin_id_hex=_to_coinset_hex(parent_coin.coin_id()),
                height=spent_height,
            )
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
        parent_puzzle_program = clvm.deserialize(_hex_to_bytes(puzzle_reveal_hex))
        parent_solution_program = clvm.deserialize(_hex_to_bytes(solution_hex))
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
