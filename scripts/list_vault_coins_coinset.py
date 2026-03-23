#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib
import json
import random
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig
from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.cloud_wallet_offer_runtime import poll_signature_request_until_not_unsigned
from greenfloor.constants import MIN_CAT_OUTPUT_MOJOS
from greenfloor.hex_utils import is_hex_id, normalize_hex_id


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


@dataclass(slots=True)
class CoinRow:
    coin_id: str
    puzzle_hash: str
    parent_coin_info: str
    amount: int
    confirmed_block_index: int
    spent_block_index: int
    discovered_nonces: list[int]
    discovered_by_puzzle_hash: bool
    discovered_by_hint: bool
    coin_type: str
    cat_asset_id: str | None
    cat_symbols: list[str]


def _is_spendable_coin_state(state: str) -> bool:
    coin_state = str(state or "").strip().upper()
    return coin_state in {"CONFIRMED", "UNSPENT", "SPENDABLE", "AVAILABLE", "SETTLED"}


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


def _launcher_from_cloud_wallet(args: argparse.Namespace) -> str:
    wallet = CloudWalletAdapter(
        CloudWalletConfig(
            base_url=args.cloud_wallet_base_url,
            user_key_id=args.cloud_wallet_user_key_id,
            private_key_pem_path=args.cloud_wallet_private_key_pem_path,
            vault_id=args.vault_id,
            network=args.network,
        )
    )
    snapshot = wallet.get_vault_custody_snapshot()
    launcher = (
        normalize_hex_id(snapshot.get("vaultLauncherId")) if isinstance(snapshot, dict) else ""
    )
    if not launcher:
        raise RuntimeError("vault_launcher_id_missing_from_cloud_wallet_snapshot")
    return launcher


def _require_cloud_wallet_adapter(args: argparse.Namespace) -> CloudWalletAdapter:
    required = [
        args.cloud_wallet_base_url,
        args.cloud_wallet_user_key_id,
        args.cloud_wallet_private_key_pem_path,
        args.vault_id,
    ]
    if any(not str(v).strip() for v in required):
        raise ValueError(
            "combine mode requires Cloud Wallet args: "
            "--cloud-wallet-base-url, --cloud-wallet-user-key-id, "
            "--cloud-wallet-private-key-pem-path, --vault-id"
        )
    return CloudWalletAdapter(
        CloudWalletConfig(
            base_url=args.cloud_wallet_base_url,
            user_key_id=args.cloud_wallet_user_key_id,
            private_key_pem_path=args.cloud_wallet_private_key_pem_path,
            vault_id=args.vault_id,
            network=args.network,
        )
    )


def _resolve_cloud_wallet_cat_global_id(wallet: CloudWalletAdapter, cat_asset_id_hex: str) -> str:
    query = """
query resolveAssetByIdentifier($identifier: String) {
  asset(identifier: $identifier) {
    id
    type
  }
}
"""
    payload = wallet._graphql(query=query, variables={"identifier": cat_asset_id_hex})  # noqa: SLF001
    asset = payload.get("asset") if isinstance(payload, dict) else None
    if not isinstance(asset, dict):
        raise RuntimeError(f"cloud_wallet_asset_lookup_failed:{cat_asset_id_hex}")
    global_id = str(asset.get("id", "")).strip()
    asset_type = str(asset.get("type", "")).strip().upper()
    if not global_id.startswith("Asset_") or asset_type not in {"CAT", "CAT2"}:
        raise RuntimeError(f"cloud_wallet_asset_lookup_not_cat:{cat_asset_id_hex}:{asset_type}")
    return global_id


def _coin_name_to_global_id_map_for_asset(
    wallet: CloudWalletAdapter,
    *,
    asset_global_id: str,
) -> dict[str, str]:
    # Prefer asset-scoped coin queries so Cloud Wallet performs server-side
    # filtering against the requested CAT and we avoid brittle row-level
    # asset-id comparisons.
    coins = wallet.list_coins(asset_id=asset_global_id, include_pending=True)
    scoped_query = True
    if not coins:
        # Fallback to unscoped listing for environments that return empty pages
        # for asset-scoped requests; retain legacy row-level asset filtering.
        coins = wallet.list_coins(include_pending=True)
        scoped_query = False
    mapping: dict[str, str] = {}
    for coin in coins:
        coin_global_id = str(coin.get("id", "")).strip()
        coin_name = normalize_hex_id(coin.get("name"))
        state = str(coin.get("state", "")).strip()
        if not coin_global_id or not coin_name:
            continue
        if not _is_spendable_coin_state(state):
            continue
        if bool(coin.get("isLocked", False)):
            continue
        if not scoped_query:
            asset = coin.get("asset")
            asset_id = str(asset.get("id", "")).strip() if isinstance(asset, dict) else ""
            if asset_id != asset_global_id:
                continue
        mapping[coin_name] = coin_global_id
    return mapping


def _combine_cat_dust(
    *,
    args: argparse.Namespace,
    wallet: CloudWalletAdapter,
    rows: list[CoinRow],
    requested_cat_ids: set[str],
) -> dict[str, Any]:
    requested_threshold = max(1, int(args.dust_threshold_mojos))
    # CAT denomination floor: never plan combines that can create sub-unit CAT outputs.
    threshold = max(MIN_CAT_OUTPUT_MOJOS, requested_threshold)
    threshold_raised = threshold != requested_threshold
    max_inputs = max(2, int(args.combine_max_inputs))
    fee_mojos = max(0, int(args.combine_fee_mojos))
    dry_run = bool(args.combine_dry_run)
    wait_for_signature = not bool(args.combine_no_wait_signature)
    signature_timeout_seconds = max(1, int(args.combine_signature_timeout_seconds))
    signature_warning_interval_seconds = max(
        1, int(args.combine_signature_warning_interval_seconds)
    )

    dust_by_asset: dict[str, list[CoinRow]] = {}
    for row in rows:
        if row.coin_type != "CAT" or not row.cat_asset_id:
            continue
        if int(row.amount) >= threshold:
            continue
        if requested_cat_ids and row.cat_asset_id not in requested_cat_ids:
            continue
        dust_by_asset.setdefault(row.cat_asset_id, []).append(row)

    operations: list[dict[str, Any]] = []
    for asset_id_hex in sorted(dust_by_asset.keys()):
        dust_rows = sorted(dust_by_asset[asset_id_hex], key=lambda c: (c.amount, c.coin_id))
        if len(dust_rows) < 2:
            operations.append(
                {
                    "cat_asset_id": asset_id_hex,
                    "status": "skipped",
                    "reason": "insufficient_dust_coins",
                    "dust_coin_count": len(dust_rows),
                }
            )
            continue
        asset_global_id = _resolve_cloud_wallet_cat_global_id(wallet, asset_id_hex)
        global_map = _coin_name_to_global_id_map_for_asset(wallet, asset_global_id=asset_global_id)
        unresolved_coin_ids = [row.coin_id for row in dust_rows if row.coin_id not in global_map]
        if unresolved_coin_ids:
            operations.append(
                {
                    "cat_asset_id": asset_id_hex,
                    "asset_global_id": asset_global_id,
                    "status": "error",
                    "reason": "coin_id_not_mappable_to_cloud_wallet_global_id",
                    "dust_coin_count": len(dust_rows),
                    "unresolved_coin_count": len(unresolved_coin_ids),
                    "unresolved_coin_ids_sample": unresolved_coin_ids[:25],
                    "unresolved_coin_ids": unresolved_coin_ids,
                    "operator_guidance": (
                        "Coinset-discovered CAT coin names did not map 1:1 to Cloud Wallet Coin_* ids. "
                        "Treat this as a wallet-vs-chain divergence signal and investigate Cloud Wallet sync/indexing."
                    ),
                }
            )
            continue

        candidate_inputs = [
            {
                "coin_name": row.coin_id,
                "coin_global_id": global_map[row.coin_id],
                "amount": int(row.amount),
            }
            for row in dust_rows
        ]
        # Build batches whose summed input amount is guaranteed to be >= CAT floor.
        # Greedy largest-first keeps batch count low and avoids emitting new dust.
        remaining = sorted(candidate_inputs, key=lambda item: int(item["amount"]))
        batch_plans: list[dict[str, Any]] = []
        while len(remaining) >= 2:
            pool = remaining[-max_inputs:]
            selected: list[dict[str, Any]] = []
            selected_total = 0
            for item in reversed(pool):
                selected.append(item)
                selected_total += int(item["amount"])
                if len(selected) >= 2 and selected_total >= threshold:
                    break
            if len(selected) < 2 or selected_total < threshold:
                break
            selected_ids = {str(item["coin_global_id"]) for item in selected}
            remaining = [
                item for item in remaining if str(item["coin_global_id"]) not in selected_ids
            ]
            batch_plans.append(
                {
                    "input_coin_ids": [str(item["coin_global_id"]) for item in selected],
                    "input_coin_count": len(selected),
                    "input_amount_total": int(selected_total),
                    "input_coin_names": [str(item["coin_name"]) for item in selected],
                }
            )
        if remaining:
            operations.append(
                {
                    "cat_asset_id": asset_id_hex,
                    "asset_global_id": asset_global_id,
                    "status": "skipped",
                    "reason": "remaining_dust_below_cat_floor",
                    "requested_threshold_mojos": int(requested_threshold),
                    "effective_threshold_mojos": int(threshold),
                    "threshold_was_raised_to_cat_floor": bool(threshold_raised),
                    "threshold_mojos": int(threshold),
                    "remaining_coin_count": len(remaining),
                    "remaining_total_mojos": int(
                        sum(int(item.get("amount", 0)) for item in remaining)
                    ),
                    "remaining_coin_names_sample": [
                        str(item.get("coin_name", "")) for item in remaining[:25]
                    ],
                }
            )
        if not batch_plans:
            continue

        if dry_run:
            operations.append(
                {
                    "cat_asset_id": asset_id_hex,
                    "asset_global_id": asset_global_id,
                    "status": "dry_run",
                    "fee_mojos": fee_mojos,
                    "requested_threshold_mojos": int(requested_threshold),
                    "effective_threshold_mojos": int(threshold),
                    "threshold_was_raised_to_cat_floor": bool(threshold_raised),
                    "dust_coin_count": len(dust_rows),
                    "batches": batch_plans,
                }
            )
            continue

        submitted_batches: list[dict[str, Any]] = []
        for batch in batch_plans:
            result = wallet.combine_coins(
                number_of_coins=int(batch["input_coin_count"]),
                fee=fee_mojos,
                largest_first=False,
                asset_id=asset_global_id,
                input_coin_ids=list(batch["input_coin_ids"]),
            )
            signature_request_id = str(result.get("signature_request_id", "")).strip()
            final_status = str(result.get("status", "")).strip().upper()
            signature_wait_events: list[dict[str, str]] = []
            if wait_for_signature and signature_request_id and final_status == "UNSIGNED":
                try:
                    polled_status, signature_wait_events = (
                        poll_signature_request_until_not_unsigned(
                            wallet=wallet,
                            signature_request_id=signature_request_id,
                            timeout_seconds=signature_timeout_seconds,
                            warning_interval_seconds=signature_warning_interval_seconds,
                        )
                    )
                    final_status = str(polled_status).strip().upper() or final_status
                except Exception as exc:  # noqa: BLE001
                    final_status = "SIGNATURE_WAIT_ERROR"
                    signature_wait_events.append(
                        {
                            "event": "signature_wait_error",
                            "message": str(exc),
                        }
                    )
            submitted_batches.append(
                {
                    **batch,
                    "signature_request_id": signature_request_id,
                    "status": final_status,
                    "initial_status": str(result.get("status", "")).strip(),
                    "waited_for_signature": wait_for_signature,
                    "signature_wait_events": signature_wait_events,
                }
            )

        operations.append(
            {
                "cat_asset_id": asset_id_hex,
                "asset_global_id": asset_global_id,
                "status": "submitted",
                "fee_mojos": fee_mojos,
                "requested_threshold_mojos": int(requested_threshold),
                "effective_threshold_mojos": int(threshold),
                "threshold_was_raised_to_cat_floor": bool(threshold_raised),
                "dust_coin_count": len(dust_rows),
                "submitted_batches": submitted_batches,
            }
        )

    return {
        "requested_threshold_mojos": int(requested_threshold),
        "effective_threshold_mojos": int(threshold),
        "threshold_was_raised_to_cat_floor": bool(threshold_raised),
        "threshold_adjustment_note": (
            "requested dust threshold was raised to CAT floor (1000 mojos)"
            if threshold_raised
            else None
        ),
        "threshold_mojos": threshold,
        "combine_max_inputs": max_inputs,
        "combine_fee_mojos": fee_mojos,
        "combine_dry_run": dry_run,
        "combine_wait_for_signature": wait_for_signature,
        "combine_signature_timeout_seconds": signature_timeout_seconds,
        "combine_signature_warning_interval_seconds": signature_warning_interval_seconds,
        "operations": operations,
    }


def _read_launcher_id_file(path: str) -> str:
    if not str(path).strip():
        return ""
    file_path = Path(path).expanduser()
    if not file_path.exists():
        return ""
    raw = file_path.read_text(encoding="utf-8").strip()
    return normalize_hex_id(raw) or ""


def _write_launcher_id_file(path: str, launcher_id: str) -> None:
    file_path = Path(path).expanduser()
    file_path.parent.mkdir(parents=True, exist_ok=True)
    file_path.write_text(f"{launcher_id}\n", encoding="utf-8")


def _clear_cache_files(paths: list[str]) -> dict[str, str]:
    results: dict[str, str] = {}
    for raw_path in paths:
        clean = str(raw_path).strip()
        if not clean:
            continue
        path = Path(clean).expanduser()
        key = str(path)
        if path.exists():
            try:
                path.unlink()
                results[key] = "deleted"
            except Exception as exc:  # noqa: BLE001
                results[key] = f"delete_failed:{exc}"
        else:
            results[key] = "not_found"
    return results


def _coin_row_to_dict(row: CoinRow) -> dict[str, Any]:
    return {
        "coin_id": row.coin_id,
        "puzzle_hash": row.puzzle_hash,
        "parent_coin_info": row.parent_coin_info,
        "amount": int(row.amount),
        "confirmed_block_index": int(row.confirmed_block_index),
        "spent_block_index": int(row.spent_block_index),
        "discovered_nonces": sorted(int(nonce) for nonce in row.discovered_nonces),
        "discovered_by_puzzle_hash": bool(row.discovered_by_puzzle_hash),
        "discovered_by_hint": bool(row.discovered_by_hint),
        "coin_type": str(row.coin_type),
        "cat_asset_id": normalize_hex_id(row.cat_asset_id) if row.cat_asset_id else None,
        "cat_symbols": [str(symbol) for symbol in row.cat_symbols],
    }


def _coin_row_from_dict(payload: dict[str, Any]) -> CoinRow | None:
    coin_id = normalize_hex_id(payload.get("coin_id"))
    if not coin_id:
        return None
    nonces_raw = payload.get("discovered_nonces")
    nonces = [int(value) for value in nonces_raw] if isinstance(nonces_raw, list) else []
    return CoinRow(
        coin_id=coin_id,
        puzzle_hash=normalize_hex_id(payload.get("puzzle_hash")) or "",
        parent_coin_info=normalize_hex_id(payload.get("parent_coin_info")) or "",
        amount=_safe_int(payload.get("amount"), default=0),
        confirmed_block_index=_safe_int(payload.get("confirmed_block_index"), default=0),
        spent_block_index=_safe_int(payload.get("spent_block_index"), default=0),
        discovered_nonces=sorted(set(nonces)),
        discovered_by_puzzle_hash=bool(payload.get("discovered_by_puzzle_hash", False)),
        discovered_by_hint=bool(payload.get("discovered_by_hint", False)),
        coin_type=str(payload.get("coin_type", "UNKNOWN")).strip().upper() or "UNKNOWN",
        cat_asset_id=normalize_hex_id(payload.get("cat_asset_id")) or None,
        cat_symbols=[
            str(symbol).strip()
            for symbol in (
                payload.get("cat_symbols") if isinstance(payload.get("cat_symbols"), list) else []
            )
            if str(symbol).strip()
        ],
    )


def _load_scan_checkpoint(
    *,
    checkpoint_file: str,
    network: str,
    launcher_id: str,
    include_spent: bool,
) -> tuple[
    int, dict[int, str], dict[str, CoinRow], dict[str, str], dict[str, dict[str, Any]], int | None
]:
    path = Path(checkpoint_file).expanduser()
    if not path.exists():
        return 0, {}, {}, {}, {}, None
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return 0, {}, {}, {}, {}, None
    if not isinstance(parsed, dict):
        return 0, {}, {}, {}, {}, None
    if normalize_hex_id(parsed.get("launcher_id")) != normalize_hex_id(launcher_id):
        return 0, {}, {}, {}, {}, None
    if str(parsed.get("network", "")).strip().lower() != str(network).strip().lower():
        return 0, {}, {}, {}, {}, None
    if bool(parsed.get("include_spent", False)) != bool(include_spent):
        return 0, {}, {}, {}, {}, None

    raw_nonce_map = parsed.get("nonce_to_p2")
    nonce_to_p2: dict[int, str] = {}
    if isinstance(raw_nonce_map, dict):
        for nonce_key, p2_hash in raw_nonce_map.items():
            try:
                nonce = int(nonce_key)
            except (TypeError, ValueError):
                continue
            clean_hash = normalize_hex_id(p2_hash)
            if clean_hash:
                nonce_to_p2[nonce] = clean_hash

    raw_rows = parsed.get("coin_rows")
    by_coin_id: dict[str, CoinRow] = {}
    if isinstance(raw_rows, list):
        for row_raw in raw_rows:
            if not isinstance(row_raw, dict):
                continue
            row = _coin_row_from_dict(row_raw)
            if row is None:
                continue
            by_coin_id[row.coin_id] = row

    raw_cat_cache = parsed.get("cat_asset_cache")
    cat_asset_cache: dict[str, str] = {}
    if isinstance(raw_cat_cache, dict):
        for coin_id_raw, asset_id_raw in raw_cat_cache.items():
            coin_id = normalize_hex_id(coin_id_raw)
            if not coin_id:
                continue
            asset_id = normalize_hex_id(asset_id_raw) or ""
            cat_asset_cache[coin_id] = asset_id

    raw_parent_lineage = parsed.get("parent_lineage_cache")
    parent_lineage_cache: dict[str, dict[str, Any]] = {}
    if isinstance(raw_parent_lineage, dict):
        for parent_id_raw, lineage_raw in raw_parent_lineage.items():
            parent_id = normalize_hex_id(parent_id_raw)
            if not parent_id or not isinstance(lineage_raw, dict):
                continue
            child_assets_raw = lineage_raw.get("child_asset_ids")
            child_assets: dict[str, str] = {}
            if isinstance(child_assets_raw, dict):
                for child_id_raw, asset_id_raw in child_assets_raw.items():
                    child_id = normalize_hex_id(child_id_raw)
                    if not child_id:
                        continue
                    child_assets[child_id] = normalize_hex_id(asset_id_raw) or ""
            parent_lineage_cache[parent_id] = {
                "spent_height": _safe_int(lineage_raw.get("spent_height"), default=0),
                "child_asset_ids": child_assets,
            }

    max_nonce_completed = _safe_int(parsed.get("max_nonce_completed"), default=-1)
    last_synced_height_raw = parsed.get("last_synced_height")
    last_synced_height = (
        _safe_int(last_synced_height_raw, default=-1) if last_synced_height_raw is not None else -1
    )
    if last_synced_height < 0:
        last_synced_height = None
    next_nonce = max(0, max_nonce_completed + 1)
    return (
        next_nonce,
        nonce_to_p2,
        by_coin_id,
        cat_asset_cache,
        parent_lineage_cache,
        last_synced_height,
    )


def _save_scan_checkpoint(
    *,
    checkpoint_file: str,
    network: str,
    launcher_id: str,
    include_spent: bool,
    max_nonce_completed: int,
    nonce_to_p2: dict[int, str],
    by_coin_id: dict[str, CoinRow],
    cat_asset_cache: dict[str, str],
    parent_lineage_cache: dict[str, dict[str, Any]],
    last_synced_height: int | None,
    scan_start_height: int | None,
    scan_end_height: int | None,
) -> None:
    path = Path(checkpoint_file).expanduser()
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "version": 1,
        "network": str(network).strip().lower(),
        "launcher_id": normalize_hex_id(launcher_id) or "",
        "include_spent": bool(include_spent),
        "max_nonce_completed": int(max_nonce_completed),
        "last_synced_height": int(last_synced_height) if last_synced_height is not None else None,
        "scan_window": {
            "start_height": int(scan_start_height) if scan_start_height is not None else None,
            "end_height": int(scan_end_height) if scan_end_height is not None else None,
        },
        "nonce_to_p2": {str(k): v for k, v in sorted(nonce_to_p2.items())},
        "coin_rows": [
            _coin_row_to_dict(row) for row in sorted(by_coin_id.values(), key=lambda r: r.coin_id)
        ],
        "cat_asset_cache": {
            coin_id: asset_id for coin_id, asset_id in sorted(cat_asset_cache.items())
        },
        "parent_lineage_cache": {
            parent_id: {
                "spent_height": _safe_int(lineage.get("spent_height"), default=0),
                "child_asset_ids": {
                    child_id: normalize_hex_id(asset_id) or ""
                    for child_id, asset_id in sorted(
                        (
                            (normalize_hex_id(raw_child_id) or "", raw_asset_id)
                            for raw_child_id, raw_asset_id in (
                                lineage.get("child_asset_ids").items()
                                if isinstance(lineage.get("child_asset_ids"), dict)
                                else []
                            )
                        ),
                        key=lambda item: item[0],
                    )
                    if child_id
                },
            }
            for parent_id, lineage in sorted(parent_lineage_cache.items())
        },
    }
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")


def _normalize_label(value: object) -> str:
    return "".join(ch for ch in str(value).strip().lower() if ch.isalnum())


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def _load_yaml_mapping(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        parsed = yaml.safe_load(handle) or {}
    if not isinstance(parsed, dict):
        return {}
    return parsed


def _load_cat_metadata_indexes() -> tuple[dict[str, set[str]], dict[str, list[str]]]:
    ticker_to_asset_ids: dict[str, set[str]] = {}
    asset_id_to_symbols: dict[str, set[str]] = {}

    def add_mapping(*, ticker: object, asset_id: object) -> None:
        clean_asset_id = normalize_hex_id(asset_id)
        clean_ticker = _normalize_label(ticker)
        if not clean_asset_id or not clean_ticker:
            return
        ticker_to_asset_ids.setdefault(clean_ticker, set()).add(clean_asset_id)
        asset_id_to_symbols.setdefault(clean_asset_id, set()).add(str(ticker).strip())

    cats_path = _repo_root() / "config" / "cats.yaml"
    if cats_path.exists():
        try:
            cats_payload = _load_yaml_mapping(cats_path)
        except Exception:
            cats_payload = {}
        cats = cats_payload.get("cats") if isinstance(cats_payload, dict) else None
        if isinstance(cats, list):
            for item in cats:
                if not isinstance(item, dict):
                    continue
                asset_id = item.get("asset_id")
                add_mapping(ticker=item.get("base_symbol"), asset_id=asset_id)
                add_mapping(ticker=item.get("name"), asset_id=asset_id)
                aliases = item.get("aliases")
                if isinstance(aliases, list):
                    for alias in aliases:
                        add_mapping(ticker=alias, asset_id=asset_id)

    markets_path = _repo_root() / "config" / "markets.yaml"
    if markets_path.exists():
        try:
            markets_payload = _load_yaml_mapping(markets_path)
        except Exception:
            markets_payload = {}
        markets = markets_payload.get("markets") if isinstance(markets_payload, dict) else None
        if isinstance(markets, list):
            for market in markets:
                if not isinstance(market, dict):
                    continue
                add_mapping(ticker=market.get("base_symbol"), asset_id=market.get("base_asset"))
                quote_asset = str(market.get("quote_asset", "")).strip()
                if is_hex_id(quote_asset):
                    add_mapping(ticker=quote_asset, asset_id=quote_asset)

    frozen_asset_to_symbols = {k: sorted(v) for k, v in asset_id_to_symbols.items()}
    return ticker_to_asset_ids, frozen_asset_to_symbols


def _parse_csv_values(values: list[str]) -> list[str]:
    parsed: list[str] = []
    for value in values:
        parts = [segment.strip() for segment in str(value).split(",")]
        parsed.extend([part for part in parts if part])
    return parsed


def _resolve_requested_cat_ids(
    *,
    cat_ids: list[str],
    cat_tickers: list[str],
    ticker_to_asset_ids: dict[str, set[str]],
) -> tuple[set[str], list[str]]:
    resolved: set[str] = set()
    unresolved_tickers: list[str] = []
    for raw_id in cat_ids:
        clean = normalize_hex_id(raw_id)
        if clean:
            resolved.add(clean)
    for ticker in cat_tickers:
        key = _normalize_label(ticker)
        matches = ticker_to_asset_ids.get(key, set())
        if not matches:
            unresolved_tickers.append(str(ticker).strip())
            continue
        resolved.update(matches)
    return resolved, unresolved_tickers


def main() -> int:
    parser = argparse.ArgumentParser(
        description="List vault coins via Coinset using only vault singleton launcher id.",
    )
    parser.add_argument("--network", default="mainnet", choices=["mainnet", "testnet11", "testnet"])
    parser.add_argument("--coinset-base-url", default="")
    parser.add_argument(
        "--launcher-id",
        default="",
        help="Optional vault launcher id hex; fetched from Cloud Wallet when omitted.",
    )
    parser.add_argument(
        "--launcher-id-file",
        default="",
        help="Read launcher id from this file when --launcher-id is omitted; fetched launcher id is saved here too.",
    )
    parser.add_argument(
        "--resolve-launcher-id-only",
        action="store_true",
        help="Resolve launcher id (from arg/file/cloud wallet), print it, then exit.",
    )
    parser.add_argument("--cloud-wallet-base-url", default="")
    parser.add_argument("--cloud-wallet-user-key-id", default="")
    parser.add_argument("--cloud-wallet-private-key-pem-path", default="")
    parser.add_argument("--vault-id", default="")
    parser.add_argument("--max-nonce", type=int, default=100)
    parser.add_argument("--include-spent", action="store_true")
    parser.add_argument("--asset-type", default="all", choices=["all", "xch", "cat"])
    parser.add_argument(
        "--cat-id",
        action="append",
        default=[],
        help="CAT asset id filter (repeat or comma-separate). Implies --asset-type cat.",
    )
    parser.add_argument(
        "--cat-ticker",
        action="append",
        default=[],
        help="CAT ticker/symbol filter from config metadata (repeat or comma-separate). Implies --asset-type cat.",
    )
    parser.add_argument("--cat-asset-id", default="")
    parser.add_argument(
        "--combine-dust",
        action="store_true",
        help="Combine CAT dust coins (< threshold mojos) using explicit Cloud Wallet inputCoinIds.",
    )
    parser.add_argument(
        "--combine-dry-run",
        action="store_true",
        help="Plan CAT dust combines but do not submit Cloud Wallet combine requests.",
    )
    parser.add_argument(
        "--dust-threshold-mojos",
        type=int,
        default=1000,
        help="Dust threshold in CAT mojos; coins with amount below this are selected (default 1000).",
    )
    parser.add_argument(
        "--combine-max-inputs",
        type=int,
        default=5,
        help="Maximum input coins per combine mutation batch (default 5).",
    )
    parser.add_argument(
        "--combine-fee-mojos",
        type=int,
        default=0,
        help="Fee mojos per combine request (default 0).",
    )
    parser.add_argument(
        "--combine-no-wait-signature",
        action="store_true",
        help="Do not wait for signature request to leave UNSIGNED after combine submission.",
    )
    parser.add_argument(
        "--combine-signature-timeout-seconds",
        type=int,
        default=15 * 60,
        help="Maximum wait time for combine signature request to leave UNSIGNED (default 900).",
    )
    parser.add_argument(
        "--combine-signature-warning-interval-seconds",
        type=int,
        default=10 * 60,
        help="Warning cadence while waiting on combine signature state (default 600).",
    )
    parser.add_argument(
        "--checkpoint-file",
        default="",
        help=(
            "Optional JSON checkpoint/cache file. "
            "When set, nonce scan progress and CAT classification cache are resumed across runs."
        ),
    )
    parser.add_argument(
        "--checkpoint-save-interval",
        type=int,
        default=1,
        help="Save checkpoint after every N nonce scans (default 1).",
    )
    parser.add_argument(
        "--no-resume-checkpoint",
        action="store_true",
        help="Ignore existing checkpoint contents and start scanning from nonce 0.",
    )
    parser.add_argument(
        "--nonce-batch-size",
        type=int,
        default=32,
        help="Nonce scan batch size for Coinset puzzle_hashes/hints queries (default 32).",
    )
    parser.add_argument(
        "--empty-batch-stop-count",
        type=int,
        default=1,
        help="Stop after this many consecutive empty nonce batches beyond nonce 0 (default 1).",
    )
    parser.add_argument(
        "--parent-lookup-batch-size",
        type=int,
        default=64,
        help="Parent coin lookup batch size for get_coin_records_by_names (default 64).",
    )
    parser.add_argument(
        "--start-height",
        type=int,
        default=None,
        help="Optional Coinset start_height filter for coin record queries.",
    )
    parser.add_argument(
        "--end-height",
        type=int,
        default=None,
        help="Optional Coinset end_height filter for coin record queries.",
    )
    parser.add_argument(
        "--incremental-from-checkpoint",
        action="store_true",
        help=(
            "When checkpointing is enabled, continue from checkpoint last_synced_height + 1 "
            "and cap end_height to current chain peak when omitted."
        ),
    )
    parser.add_argument(
        "--auto-increment",
        action="store_true",
        help=(
            "Convenience mode: enables checkpointing and incremental-from-checkpoint. "
            "Uses ~/.greenfloor/cache/vault_coinset_checkpoint.json when --checkpoint-file is unset."
        ),
    )
    parser.add_argument(
        "--clear-caches",
        action="store_true",
        help=(
            "Clear launcher/checkpoint cache files before scanning. "
            "Targets launcher-id-file/checkpoint-file when provided, otherwise default cache paths."
        ),
    )
    args = parser.parse_args()

    if bool(args.auto_increment):
        if bool(args.no_resume_checkpoint):
            raise ValueError("cannot use --auto-increment with --no-resume-checkpoint")
        if not str(args.checkpoint_file).strip():
            args.checkpoint_file = "~/.greenfloor/cache/vault_coinset_checkpoint.json"
        args.incremental_from_checkpoint = True
    if bool(args.clear_caches):
        cache_files = [
            str(args.launcher_id_file).strip() or "~/.greenfloor/cache/vault_launcher_id.txt",
            str(args.checkpoint_file).strip()
            or "~/.greenfloor/cache/vault_coinset_checkpoint.json",
        ]
        cache_clear_result = _clear_cache_files(cache_files)
    else:
        cache_clear_result = {}

    launcher_id = normalize_hex_id(args.launcher_id)
    launcher_id_source = "arg"
    if not launcher_id and str(args.launcher_id_file).strip():
        launcher_id = _read_launcher_id_file(args.launcher_id_file)
        if launcher_id:
            launcher_id_source = "file"
    if not launcher_id:
        required = [
            args.cloud_wallet_base_url,
            args.cloud_wallet_user_key_id,
            args.cloud_wallet_private_key_pem_path,
            args.vault_id,
        ]
        if any(not str(v).strip() for v in required):
            raise ValueError(
                "launcher-id, launcher-id-file, or full Cloud Wallet auth args are required"
            )
        launcher_id = _launcher_from_cloud_wallet(args)
        launcher_id_source = "cloud_wallet"
    if str(args.launcher_id_file).strip() and launcher_id_source in {"cloud_wallet", "arg"}:
        _write_launcher_id_file(args.launcher_id_file, launcher_id)

    if bool(args.resolve_launcher_id_only):
        print(
            json.dumps(
                {
                    "launcher_id": launcher_id,
                    "launcher_id_source": launcher_id_source,
                    "launcher_id_file": str(Path(args.launcher_id_file).expanduser())
                    if str(args.launcher_id_file).strip()
                    else None,
                },
                indent=2,
            )
        )
        return 0

    sdk = _import_sdk()
    scanner = CoinsetScanner(network=args.network, base_url=args.coinset_base_url or None)
    ticker_to_asset_ids, asset_id_to_symbols = _load_cat_metadata_indexes()
    requested_cat_ids_raw = _parse_csv_values(args.cat_id)
    requested_cat_tickers_raw = _parse_csv_values(args.cat_ticker)
    if str(args.cat_asset_id).strip():
        requested_cat_ids_raw.append(str(args.cat_asset_id).strip())
    requested_cat_ids, unresolved_cat_tickers = _resolve_requested_cat_ids(
        cat_ids=requested_cat_ids_raw,
        cat_tickers=requested_cat_tickers_raw,
        ticker_to_asset_ids=ticker_to_asset_ids,
    )
    if unresolved_cat_tickers:
        raise ValueError(f"unknown cat ticker(s): {', '.join(unresolved_cat_tickers)}")
    effective_asset_type = (
        "cat"
        if requested_cat_ids or requested_cat_tickers_raw
        else str(args.asset_type).strip().lower()
    )

    max_nonce_target = max(0, int(args.max_nonce))
    checkpoint_file = str(args.checkpoint_file).strip()
    checkpoint_save_interval = max(1, int(args.checkpoint_save_interval))
    checkpoint_enabled = bool(checkpoint_file)
    checkpoint_resumed = False
    checkpoint_start_nonce = 0

    by_coin_id: dict[str, CoinRow]
    nonce_to_p2: dict[int, str]
    cat_asset_cache: dict[str, str]
    parent_lineage_cache: dict[str, dict[str, Any]]
    checkpoint_last_synced_height: int | None
    if checkpoint_enabled and not bool(args.no_resume_checkpoint):
        (
            checkpoint_start_nonce,
            nonce_to_p2,
            by_coin_id,
            cat_asset_cache,
            parent_lineage_cache,
            checkpoint_last_synced_height,
        ) = _load_scan_checkpoint(
            checkpoint_file=checkpoint_file,
            network=args.network,
            launcher_id=launcher_id,
            include_spent=bool(args.include_spent),
        )
        checkpoint_resumed = (
            checkpoint_start_nonce > 0
            or bool(by_coin_id)
            or bool(cat_asset_cache)
            or bool(parent_lineage_cache)
        )
    else:
        by_coin_id = {}
        nonce_to_p2 = {}
        cat_asset_cache = {}
        parent_lineage_cache = {}
        checkpoint_last_synced_height = None

    nonce_batch_size = max(1, int(args.nonce_batch_size))
    empty_batch_stop_count = max(1, int(args.empty_batch_stop_count))
    parent_lookup_batch_size = max(1, int(args.parent_lookup_batch_size))
    requested_start_height = (
        args.start_height if args.start_height is None else max(0, int(args.start_height))
    )
    requested_end_height = (
        args.end_height if args.end_height is None else max(0, int(args.end_height))
    )
    if requested_start_height is not None and requested_end_height is not None:
        if requested_end_height < requested_start_height:
            raise ValueError("end-height must be greater than or equal to start-height")
    if bool(args.incremental_from_checkpoint) and requested_start_height is not None:
        raise ValueError("cannot use --start-height with --incremental-from-checkpoint")
    if bool(args.incremental_from_checkpoint) and not checkpoint_enabled:
        raise ValueError("--incremental-from-checkpoint requires --checkpoint-file")

    chain_peak_height: int | None = None
    if bool(args.incremental_from_checkpoint) or requested_end_height is None:
        state = _coinset_with_retries(lambda: scanner.adapter.get_blockchain_state())
        if isinstance(state, dict):
            peak_raw = state.get("peak")
            if isinstance(peak_raw, dict):
                chain_peak_height = _safe_int(peak_raw.get("height"), default=-1)
            if chain_peak_height is None or chain_peak_height < 0:
                chain_peak_height = _safe_int(state.get("peak_height"), default=-1)
            if chain_peak_height is not None and chain_peak_height < 0:
                chain_peak_height = None

    effective_start_height = requested_start_height
    if bool(args.incremental_from_checkpoint):
        if checkpoint_last_synced_height is not None and checkpoint_last_synced_height >= 0:
            effective_start_height = int(checkpoint_last_synced_height) + 1
        elif effective_start_height is None:
            effective_start_height = 0
    effective_end_height = requested_end_height
    if effective_end_height is None and chain_peak_height is not None:
        effective_end_height = int(chain_peak_height)
    checkpoint_synced_height = (
        int(effective_end_height)
        if effective_end_height is not None
        else (int(chain_peak_height) if chain_peak_height is not None else None)
    )

    if (
        effective_start_height is not None
        and effective_end_height is not None
        and effective_start_height > effective_end_height
    ):
        stop_reason = "scan_window_exhausted"
        print(
            json.dumps(
                {
                    "network": scanner.adapter.network,
                    "coinset_base_url": scanner.adapter.base_url,
                    "launcher_id": launcher_id,
                    "asset_type": effective_asset_type,
                    "requested_cat_ids": sorted(requested_cat_ids),
                    "requested_cat_tickers": sorted(set(requested_cat_tickers_raw)),
                    "max_nonce_scanned": max(nonce_to_p2.keys()) if nonce_to_p2 else 0,
                    "count": 0,
                    "checkpoint": {
                        "enabled": checkpoint_enabled,
                        "file": str(Path(checkpoint_file).expanduser())
                        if checkpoint_enabled
                        else None,
                        "resumed": checkpoint_resumed,
                        "start_nonce": checkpoint_start_nonce,
                        "save_interval": checkpoint_save_interval if checkpoint_enabled else None,
                        "cat_asset_cache_entries": len(cat_asset_cache),
                        "parent_lineage_cache_entries": len(parent_lineage_cache),
                        "last_synced_height": checkpoint_last_synced_height,
                    },
                    "scan_batches": {
                        "nonce_batch_size": nonce_batch_size,
                        "empty_batch_stop_count": empty_batch_stop_count,
                        "parent_lookup_batch_size": parent_lookup_batch_size,
                    },
                    "scan_window": {
                        "start_height": effective_start_height,
                        "end_height": effective_end_height,
                        "chain_peak_height": chain_peak_height,
                        "incremental_from_checkpoint": bool(args.incremental_from_checkpoint),
                        "auto_increment": bool(args.auto_increment),
                    },
                    "scan_stop_reason": stop_reason,
                    "combine_dust": None,
                    "coins": [],
                },
                indent=2,
            )
        )
        return 0

    scanned_since_resume = 0
    empty_batch_count = 0
    stop_reason = "max_nonce_reached"
    for batch_start in range(checkpoint_start_nonce, max_nonce_target + 1, nonce_batch_size):
        batch_end = min(batch_start + nonce_batch_size - 1, max_nonce_target)
        batch_nonces = list(range(batch_start, batch_end + 1))
        nonce_p2: dict[int, str] = {}
        for nonce in batch_nonces:
            cfg = sdk.MemberConfig().with_top_level(True).with_nonce(int(nonce))
            p2_hash = normalize_hex_id(
                sdk.to_hex(sdk.singleton_member_hash(cfg, _hex_to_bytes(launcher_id), False))
            )
            if p2_hash:
                nonce_p2[nonce] = p2_hash
                nonce_to_p2[nonce] = p2_hash
        p2_hashes = list(
            dict.fromkeys(_to_coinset_hex(_hex_to_bytes(v)) for v in nonce_p2.values())
        )
        by_puzzle = scanner.by_puzzle_hashes(
            puzzle_hashes=p2_hashes,
            include_spent=args.include_spent,
            start_height=effective_start_height,
            end_height=effective_end_height,
        )
        by_hint = scanner.by_hints(
            hints=p2_hashes,
            include_spent=args.include_spent,
            start_height=effective_start_height,
            end_height=effective_end_height,
        )
        batch_has_any = bool(by_puzzle) or bool(by_hint)
        if batch_end > 0 and not batch_has_any:
            empty_batch_count += 1
        else:
            empty_batch_count = 0
        if empty_batch_count >= empty_batch_stop_count:
            stop_reason = "empty_nonce_batches"
            if checkpoint_enabled:
                _save_scan_checkpoint(
                    checkpoint_file=checkpoint_file,
                    network=args.network,
                    launcher_id=launcher_id,
                    include_spent=bool(args.include_spent),
                    max_nonce_completed=batch_end,
                    nonce_to_p2=nonce_to_p2,
                    by_coin_id=by_coin_id,
                    cat_asset_cache=cat_asset_cache,
                    parent_lineage_cache=parent_lineage_cache,
                    last_synced_height=checkpoint_synced_height,
                    scan_start_height=effective_start_height,
                    scan_end_height=effective_end_height,
                )
            break

        for source, records in (("puzzle_hash", by_puzzle), ("hint", by_hint)):
            for record in records:
                coin_id = _coin_id_from_record(record)
                if not coin_id:
                    continue
                coin_raw = record.get("coin")
                coin: dict[str, Any] = coin_raw if isinstance(coin_raw, dict) else {}
                row = by_coin_id.get(coin_id)
                if row is None:
                    row = CoinRow(
                        coin_id=coin_id,
                        puzzle_hash=normalize_hex_id(coin.get("puzzle_hash")) or "",
                        parent_coin_info=normalize_hex_id(coin.get("parent_coin_info")) or "",
                        amount=_safe_int(coin.get("amount"), default=0),
                        confirmed_block_index=_safe_int(
                            record.get("confirmed_block_index"), default=0
                        ),
                        spent_block_index=_safe_int(record.get("spent_block_index"), default=0),
                        discovered_nonces=[],
                        discovered_by_puzzle_hash=False,
                        discovered_by_hint=False,
                        coin_type="UNKNOWN",
                        cat_asset_id=None,
                        cat_symbols=[],
                    )
                    by_coin_id[coin_id] = row
                for nonce, batch_p2 in nonce_p2.items():
                    if row.puzzle_hash == batch_p2 and nonce not in row.discovered_nonces:
                        row.discovered_nonces.append(nonce)
                row.discovered_nonces.sort()
                if source == "puzzle_hash":
                    row.discovered_by_puzzle_hash = True
                if source == "hint":
                    row.discovered_by_hint = True

        scanned_since_resume += len(batch_nonces)
        if checkpoint_enabled and (
            scanned_since_resume % checkpoint_save_interval == 0 or batch_end >= max_nonce_target
        ):
            _save_scan_checkpoint(
                checkpoint_file=checkpoint_file,
                network=args.network,
                launcher_id=launcher_id,
                include_spent=bool(args.include_spent),
                max_nonce_completed=batch_end,
                nonce_to_p2=nonce_to_p2,
                by_coin_id=by_coin_id,
                cat_asset_cache=cat_asset_cache,
                parent_lineage_cache=parent_lineage_cache,
                last_synced_height=checkpoint_synced_height,
                scan_start_height=effective_start_height,
                scan_end_height=effective_end_height,
            )

    parent_record_cache: dict[str, dict[str, Any] | None] = {}
    puzzle_solution_cache: dict[str, dict[str, Any] | None] = {}
    unresolved_parent_ids = sorted(
        {
            row.parent_coin_info
            for row in by_coin_id.values()
            if row.parent_coin_info and row.parent_coin_info not in parent_record_cache
        }
    )
    for parent_batch in _chunk_values(unresolved_parent_ids, parent_lookup_batch_size):
        parent_records = scanner.by_names(
            coin_names=[_to_coinset_hex(_hex_to_bytes(parent_id)) for parent_id in parent_batch],
            include_spent=True,
        )
        for parent in parent_records:
            parent_id = _coin_id_from_record(parent)
            if parent_id:
                parent_record_cache[parent_id] = parent

    for row in by_coin_id.values():
        p2_hashes = {nonce_to_p2.get(nonce, "") for nonce in row.discovered_nonces}
        if row.puzzle_hash and row.puzzle_hash in p2_hashes:
            row.coin_type = "XCH"
            continue
        cached_asset_id = cat_asset_cache.get(row.coin_id)
        if cached_asset_id is not None:
            if cached_asset_id:
                row.coin_type = "CAT"
                row.cat_asset_id = cached_asset_id
                row.cat_symbols = list(asset_id_to_symbols.get(cached_asset_id, []))
            else:
                row.coin_type = "OTHER"
            continue
        record = {
            "coin": {
                "parent_coin_info": row.parent_coin_info,
                "puzzle_hash": row.puzzle_hash,
                "amount": row.amount,
            },
        }
        cat_asset_id = _detect_cat_asset_id(
            sdk=sdk,
            coinset=scanner,
            coin_id=row.coin_id,
            record=record,
            cat_asset_cache=cat_asset_cache,
            parent_record_cache=parent_record_cache,
            puzzle_solution_cache=puzzle_solution_cache,
            parent_lineage_cache=parent_lineage_cache,
        )
        if cat_asset_id:
            row.coin_type = "CAT"
            row.cat_asset_id = cat_asset_id
            row.cat_symbols = list(asset_id_to_symbols.get(cat_asset_id, []))
            continue
        row.coin_type = "OTHER"

    max_nonce_scanned = max(nonce_to_p2.keys()) if nonce_to_p2 else 0
    pre_verify_count = len(by_coin_id)
    verified_coin_ids = scanner.existing_coin_names(coin_ids_hex=sorted(by_coin_id.keys()))
    verification_applied = bool(verified_coin_ids)
    if verification_applied:
        by_coin_id = {
            coin_id: row for coin_id, row in by_coin_id.items() if coin_id in verified_coin_ids
        }
    dropped_unverified_count = (
        max(0, pre_verify_count - len(by_coin_id)) if verification_applied else 0
    )
    if checkpoint_enabled:
        _save_scan_checkpoint(
            checkpoint_file=checkpoint_file,
            network=args.network,
            launcher_id=launcher_id,
            include_spent=bool(args.include_spent),
            max_nonce_completed=max_nonce_scanned,
            nonce_to_p2=nonce_to_p2,
            by_coin_id=by_coin_id,
            cat_asset_cache=cat_asset_cache,
            parent_lineage_cache=parent_lineage_cache,
            last_synced_height=checkpoint_synced_height,
            scan_start_height=effective_start_height,
            scan_end_height=effective_end_height,
        )

    filtered: list[CoinRow] = []
    for row in sorted(by_coin_id.values(), key=lambda r: (r.coin_type, r.amount, r.coin_id)):
        if effective_asset_type == "xch" and row.coin_type != "XCH":
            continue
        if effective_asset_type == "cat" and row.coin_type != "CAT":
            continue
        if requested_cat_ids and (row.cat_asset_id or "") not in requested_cat_ids:
            continue
        filtered.append(row)

    combine_plan: dict[str, Any] | None = None
    if bool(args.combine_dust):
        wallet = _require_cloud_wallet_adapter(args)
        combine_plan = _combine_cat_dust(
            args=args,
            wallet=wallet,
            rows=filtered,
            requested_cat_ids=requested_cat_ids,
        )

    print(
        json.dumps(
            {
                "network": scanner.adapter.network,
                "coinset_base_url": scanner.adapter.base_url,
                "launcher_id": launcher_id,
                "asset_type": effective_asset_type,
                "requested_cat_ids": sorted(requested_cat_ids),
                "requested_cat_tickers": sorted(set(requested_cat_tickers_raw)),
                "max_nonce_scanned": max_nonce_scanned,
                "count": len(filtered),
                "name_verification": {
                    "applied": verification_applied,
                    "pre_verify_count": pre_verify_count,
                    "verified_count": len(by_coin_id) if verification_applied else None,
                    "dropped_unverified_count": dropped_unverified_count,
                },
                "cache_clear": cache_clear_result or None,
                "checkpoint": {
                    "enabled": checkpoint_enabled,
                    "file": str(Path(checkpoint_file).expanduser()) if checkpoint_enabled else None,
                    "resumed": checkpoint_resumed,
                    "start_nonce": checkpoint_start_nonce,
                    "save_interval": checkpoint_save_interval if checkpoint_enabled else None,
                    "cat_asset_cache_entries": len(cat_asset_cache),
                    "parent_lineage_cache_entries": len(parent_lineage_cache),
                    "last_synced_height": checkpoint_synced_height,
                },
                "scan_batches": {
                    "nonce_batch_size": nonce_batch_size,
                    "empty_batch_stop_count": empty_batch_stop_count,
                    "parent_lookup_batch_size": parent_lookup_batch_size,
                },
                "scan_window": {
                    "start_height": effective_start_height,
                    "end_height": effective_end_height,
                    "chain_peak_height": chain_peak_height,
                    "incremental_from_checkpoint": bool(args.incremental_from_checkpoint),
                    "auto_increment": bool(args.auto_increment),
                },
                "scan_stop_reason": stop_reason,
                "combine_dust": combine_plan,
                "coins": [
                    {
                        "coin_id": row.coin_id,
                        "type": row.coin_type,
                        "cat_asset_id": row.cat_asset_id,
                        "cat_symbols": row.cat_symbols,
                        "amount": row.amount,
                        "confirmed_block_index": row.confirmed_block_index,
                        "spent_block_index": row.spent_block_index,
                        "discovered_nonces": row.discovered_nonces,
                        "discovered_by_puzzle_hash": row.discovered_by_puzzle_hash,
                        "discovered_by_hint": row.discovered_by_hint,
                        "puzzle_hash": row.puzzle_hash,
                        "parent_coin_info": row.parent_coin_info,
                    }
                    for row in filtered
                ],
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
