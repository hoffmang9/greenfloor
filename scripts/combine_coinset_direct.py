#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import time
from dataclasses import dataclass
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig
from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.adapters.kms_signer import get_public_key_compressed_hex, sign_digest
from greenfloor.hex_utils import normalize_hex_id
from greenfloor.signing import sign_and_broadcast_mixed_split

_MIN_CAT_OUTPUT_MOJOS = 1000


@dataclass(frozen=True, slots=True)
class InputCoin:
    coin_id: str
    amount: int


def _safe_int(value: object, default: int = 0) -> int:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return default


def _parse_csv_values(values: list[str]) -> list[str]:
    parsed: list[str] = []
    for value in values:
        parts = [segment.strip() for segment in str(value).split(",")]
        parsed.extend([part for part in parts if part])
    return parsed


def _normalize_coin_names(values: list[str]) -> list[str]:
    normalized: list[str] = []
    seen: set[str] = set()
    for value in values:
        clean = normalize_hex_id(value)
        if not clean or clean in seen:
            continue
        seen.add(clean)
        normalized.append(clean)
    return normalized


def _select_input_coin_ids(coin_ids: list[str], max_input_coins: int) -> list[str]:
    max_inputs = max(2, int(max_input_coins))
    return list(coin_ids[:max_inputs])


def _min_coin_count_for_target_mojos(*, amounts: list[int], target_mojos: int) -> int | None:
    running = 0
    for idx, amount in enumerate(
        sorted((int(a) for a in amounts if int(a) > 0), reverse=True), start=1
    ):
        running += int(amount)
        if running >= int(target_mojos):
            return idx
    return None


def _partition_stepwise_chunks(coin_ids: list[str], *, max_input_coins: int) -> list[list[str]]:
    max_inputs = max(2, int(max_input_coins))
    remaining = list(coin_ids)
    chunks: list[list[str]] = []
    while len(remaining) >= 2:
        take = min(max_inputs, len(remaining))
        if len(remaining) - take == 1:
            take = max(2, take - 1)
        chunk = list(remaining[:take])
        remaining = remaining[take:]
        if len(chunk) >= 2:
            chunks.append(chunk)
    return chunks


def _to_coinset_hex(value: str) -> str:
    return f"0x{value}"


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
    return ""


def _coin_amount_from_record(record: dict[str, Any]) -> int:
    coin = record.get("coin")
    if not isinstance(coin, dict):
        return 0
    amount = _safe_int(coin.get("amount"), default=0)
    return amount if amount > 0 else 0


def _coin_spent_height_from_record(record: dict[str, Any]) -> int:
    return max(0, _safe_int(record.get("spent_block_index"), default=0))


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


def _coinset_records_by_name(
    *,
    coinset: CoinsetAdapter,
    coin_ids: list[str],
    include_spent_coins: bool,
) -> dict[str, dict[str, Any]]:
    rows = coinset.get_coin_records_by_names(
        coin_names_hex=[_to_coinset_hex(coin_id) for coin_id in coin_ids],
        include_spent_coins=include_spent_coins,
    )
    result: dict[str, dict[str, Any]] = {}
    for row in rows:
        coin_id = _coin_id_from_record(row)
        if coin_id:
            result[coin_id] = row
    # Some Coinset hosts can return partial/empty batch-name results even when
    # single-name lookups succeed. Fill gaps with per-name fallback.
    missing_ids = [coin_id for coin_id in coin_ids if coin_id not in result]
    for coin_id in missing_ids:
        row = coinset.get_coin_record_by_name(coin_name_hex=_to_coinset_hex(coin_id))
        if not isinstance(row, dict):
            continue
        resolved_id = _coin_id_from_record(row) or coin_id
        result[resolved_id] = row
    return result


def _resolve_input_coins(
    *, coinset: CoinsetAdapter, coin_ids: list[str]
) -> tuple[list[InputCoin], list[str]]:
    records_by_id = _coinset_records_by_name(
        coinset=coinset,
        coin_ids=coin_ids,
        include_spent_coins=True,
    )
    resolved: list[InputCoin] = []
    issues: list[str] = []
    for coin_id in coin_ids:
        record = records_by_id.get(coin_id)
        if not isinstance(record, dict):
            issues.append(f"coin_not_found:{coin_id}")
            continue
        spent_height = _coin_spent_height_from_record(record)
        if spent_height > 0:
            issues.append(f"coin_already_spent:{coin_id}:{spent_height}")
            continue
        amount = _coin_amount_from_record(record)
        if amount <= 0:
            issues.append(f"coin_non_positive_amount:{coin_id}")
            continue
        resolved.append(InputCoin(coin_id=coin_id, amount=amount))
    return resolved, issues


def _resolve_cat_asset_id_for_coin_ids(
    *,
    network: str,
    coin_ids: list[str],
    max_attempts: int = 4,
    retry_sleep_seconds: float = 1.0,
    sleep_fn: Any = time.sleep,
) -> tuple[str | None, dict[str, Any]]:
    import greenfloor.signing as signing_mod

    sdk = signing_mod._import_sdk()  # noqa: SLF001
    requested_ids = [coin_id for coin_id in coin_ids if coin_id]
    requested_set = set(requested_ids)
    if not requested_ids:
        return None, {"ok": False, "reason": "no_coin_ids_requested"}
    attempts = max(1, int(max_attempts))

    def _cat_coin_id_hex(cat: Any) -> str:
        coin = getattr(cat, "coin", None)
        if coin is None:
            return ""
        raw_coin_id = getattr(coin, "coin_id", None)
        if callable(raw_coin_id):
            raw_coin_id = raw_coin_id()
        try:
            return normalize_hex_id(sdk.to_hex(raw_coin_id))
        except Exception:
            return ""

    cats: list[Any] = []
    last_exception: str | None = None
    for attempt in range(1, attempts + 1):
        try:
            cats = signing_mod._list_unspent_cat_coins_by_ids(  # noqa: SLF001
                sdk=sdk,
                network=network,
                coin_ids=requested_ids,
            )
            last_exception = None
        except Exception as exc:  # noqa: BLE001
            cats = []
            last_exception = str(exc)
        resolved_ids = {_cat_coin_id_hex(cat) for cat in cats if _cat_coin_id_hex(cat)}
        missing_ids = sorted(requested_set - resolved_ids)
        if not last_exception and not missing_ids:
            break
        if attempt < attempts:
            sleep_fn(max(0.0, float(retry_sleep_seconds)))
    else:
        resolved_ids = {_cat_coin_id_hex(cat) for cat in cats if _cat_coin_id_hex(cat)}
        missing_ids = sorted(requested_set - resolved_ids)
        payload: dict[str, Any] = {
            "ok": False,
            "reason": "coinset_ids_not_all_resolved_as_unspent_cat",
            "resolved_cat_count": len(cats),
            "requested_count": len(requested_ids),
            "missing_coin_ids": missing_ids,
            "resolved_coin_ids": sorted(resolved_ids),
            "max_attempts": attempts,
        }
        if last_exception:
            payload["last_exception"] = last_exception
            payload["reason"] = "coinset_cat_resolution_error"
        return None, payload

    asset_ids: list[str] = []
    for cat in cats:
        raw = normalize_hex_id(sdk.to_hex(cat.info.asset_id))
        if not raw:
            return None, {
                "ok": False,
                "reason": "cat_asset_id_unavailable",
            }
        asset_ids.append(raw)
    unique = sorted(set(asset_ids))
    if len(unique) != 1:
        return None, {
            "ok": False,
            "reason": "multiple_cat_assets_in_selection",
            "asset_ids": unique,
        }
    return unique[0], {
        "ok": True,
        "asset_id": unique[0],
        "resolved_cat_count": len(cats),
        "requested_count": len(requested_ids),
        "max_attempts": attempts,
    }


def _kms_resolution_check(
    *,
    kms_key_id: str,
    kms_region: str,
    kms_live_probe: bool,
    live_probe_message_hex: str,
    kms_pubkey_resolver: Any = get_public_key_compressed_hex,
    kms_signer: Any = sign_digest,
) -> dict[str, Any]:
    result: dict[str, Any] = {
        "ok": False,
        "live_probe_requested": bool(kms_live_probe),
        "live_probe_ran": False,
    }
    pubkey = str(kms_pubkey_resolver(kms_key_id, kms_region)).strip().lower()
    result["public_key_hex_prefix"] = pubkey[:16]
    if len(pubkey) != 66:
        result["reason"] = "invalid_kms_public_key_length"
        return result
    if not kms_live_probe:
        result["ok"] = True
        return result
    signature_hex = str(kms_signer(kms_key_id, kms_region, live_probe_message_hex)).strip().lower()
    result["live_probe_ran"] = True
    result["live_probe_signature_prefix"] = signature_hex[:16]
    if len(signature_hex) != 128:
        result["reason"] = "invalid_kms_signature_length"
        return result
    result["ok"] = True
    return result


def _preflight_checks(
    *,
    args: argparse.Namespace,
    coinset: CoinsetAdapter,
    selected_coin_ids: list[str],
    total_amount: int,
    asset_id: str | None,
    cloud_wallet_factory: Any = CloudWalletAdapter,
    kms_pubkey_resolver: Any = get_public_key_compressed_hex,
    kms_signer: Any = sign_digest,
) -> dict[str, Any]:
    checks: dict[str, Any] = {}

    try:
        blockchain_state = coinset.get_blockchain_state()
        checks["coinset"] = {
            "ok": isinstance(blockchain_state, dict),
            "base_url": coinset.base_url,
            "network": coinset.network,
        }
    except Exception as exc:  # noqa: BLE001
        checks["coinset"] = {"ok": False, "reason": str(exc), "base_url": coinset.base_url}

    try:
        wallet = cloud_wallet_factory(
            CloudWalletConfig(
                base_url=str(args.cloud_wallet_base_url).strip(),
                user_key_id=str(args.cloud_wallet_user_key_id).strip(),
                private_key_pem_path=str(args.cloud_wallet_private_key_pem_path).strip(),
                vault_id=str(args.vault_id).strip(),
                network=str(args.network).strip(),
                kms_key_id=str(args.cloud_wallet_kms_key_id).strip() or None,
                kms_region=str(args.cloud_wallet_kms_region).strip() or None,
                kms_public_key_hex=str(args.cloud_wallet_kms_public_key_hex).strip() or None,
            )
        )
        snapshot = wallet.get_vault_custody_snapshot()
        launcher_id = (
            normalize_hex_id(snapshot.get("vaultLauncherId")) if isinstance(snapshot, dict) else ""
        )
        checks["cloud_wallet_snapshot"] = {
            "ok": bool(launcher_id),
            "launcher_id": launcher_id or None,
        }
    except Exception as exc:  # noqa: BLE001
        checks["cloud_wallet_snapshot"] = {"ok": False, "reason": str(exc)}

    try:
        checks["kms_resolution"] = _kms_resolution_check(
            kms_key_id=str(args.cloud_wallet_kms_key_id).strip(),
            kms_region=str(args.cloud_wallet_kms_region).strip() or "us-west-2",
            kms_live_probe=bool(args.kms_live_probe),
            live_probe_message_hex=str(args.kms_live_probe_message_hex).strip(),
            kms_pubkey_resolver=kms_pubkey_resolver,
            kms_signer=kms_signer,
        )
    except Exception as exc:  # noqa: BLE001
        checks["kms_resolution"] = {
            "ok": False,
            "live_probe_requested": bool(args.kms_live_probe),
            "live_probe_ran": bool(args.kms_live_probe),
            "reason": str(exc),
        }

    checks["payload_validation"] = {
        "ok": bool(asset_id) and len(selected_coin_ids) >= 2 and int(total_amount) > 0,
        "asset_id": asset_id,
        "input_coin_count": len(selected_coin_ids),
        "output_amount": int(total_amount),
    }
    ready = all(bool(check.get("ok", False)) for check in checks.values())
    return {"ready": ready, "checks": checks}


def _build_signing_payload(
    *,
    args: argparse.Namespace,
    selected_coin_ids: list[str],
    asset_id: str,
    output_amount: int,
) -> dict[str, Any]:
    return {
        "key_id": str(args.key_id).strip(),
        "network": str(args.network).strip(),
        "receive_address": str(args.receive_address).strip(),
        "keyring_yaml_path": str(args.keyring_yaml_path).strip(),
        "asset_id": asset_id,
        "selected_coin_ids": list(selected_coin_ids),
        "output_amounts_base_units": [int(output_amount)],
        "fee_mojos": 0,
        "cloud_wallet_base_url": str(args.cloud_wallet_base_url).strip(),
        "cloud_wallet_user_key_id": str(args.cloud_wallet_user_key_id).strip(),
        "cloud_wallet_private_key_pem_path": str(args.cloud_wallet_private_key_pem_path).strip(),
        "cloud_wallet_vault_id": str(args.vault_id).strip(),
        "cloud_wallet_kms_key_id": str(args.cloud_wallet_kms_key_id).strip(),
        "cloud_wallet_kms_region": str(args.cloud_wallet_kms_region).strip(),
        "cloud_wallet_kms_public_key_hex": str(args.cloud_wallet_kms_public_key_hex).strip(),
        "cloud_wallet_vault_nonce_probe_max": max(
            0, int(getattr(args, "cloud_wallet_vault_nonce_probe_max", 2048))
        ),
    }


def _try_get_mempool_items_by_coin_name(*, coinset: CoinsetAdapter, coin_id: str) -> dict[str, Any]:
    try:
        payload = coinset._post_json(  # noqa: SLF001
            "get_mempool_items_by_coin_name",
            {"coin_name": _to_coinset_hex(coin_id)},
        )
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "reason": str(exc)}
    if not isinstance(payload, dict):
        return {"ok": False, "reason": "invalid_payload"}
    if not bool(payload.get("success", False)):
        return {"ok": False, "reason": str(payload.get("error") or "unknown")}
    items = payload.get("mempool_items")
    if not isinstance(items, dict):
        return {"ok": True, "item_count": 0, "tx_ids": []}
    tx_ids = list(items.keys())
    return {"ok": True, "item_count": len(tx_ids), "tx_ids": tx_ids[:25]}


def _build_broadcast_diagnostics(
    *,
    signing_payload: dict[str, Any],
    coinset: CoinsetAdapter,
    input_coin_ids: list[str],
) -> dict[str, Any]:
    import greenfloor.signing as signing_mod

    diagnostics: dict[str, Any] = {}
    try:
        spend_bundle_hex, bundle_error = signing_mod._build_mixed_split_spend_bundle(
            signing_payload
        )  # noqa: SLF001
    except Exception as exc:  # noqa: BLE001
        diagnostics["bundle_build"] = {"ok": False, "reason": f"build_exception:{exc}"}
        return diagnostics
    if not spend_bundle_hex:
        diagnostics["bundle_build"] = {"ok": False, "reason": str(bundle_error or "unknown")}
        return diagnostics
    diagnostics["bundle_build"] = {"ok": True}
    diagnostics["spend_bundle_hex_prefix"] = str(spend_bundle_hex)[:120]
    diagnostics["spend_bundle_hex_length"] = len(str(spend_bundle_hex))

    try:
        sdk = signing_mod._import_sdk()  # noqa: SLF001
        raw_hex = (
            spend_bundle_hex[2:]
            if str(spend_bundle_hex).lower().startswith("0x")
            else str(spend_bundle_hex)
        )
        spend_bundle = sdk.SpendBundle.from_bytes(bytes.fromhex(raw_hex))
        diagnostics["tx_id"] = sdk.to_hex(spend_bundle.hash())
        coin_spends = getattr(spend_bundle, "coin_spends", None) or []
        diagnostics["coin_spend_count"] = len(coin_spends)
    except Exception as exc:  # noqa: BLE001
        diagnostics["bundle_decode"] = {"ok": False, "reason": str(exc)}

    records = _coinset_records_by_name(
        coinset=coinset,
        coin_ids=input_coin_ids,
        include_spent_coins=True,
    )
    diagnostics["input_coin_states"] = {
        coin_id: {
            "found": coin_id in records,
            "spent_block_index": _coin_spent_height_from_record(records.get(coin_id, {}))
            if coin_id in records
            else None,
        }
        for coin_id in input_coin_ids
    }
    diagnostics["mempool_probe"] = {
        coin_id: _try_get_mempool_items_by_coin_name(coinset=coinset, coin_id=coin_id)
        for coin_id in input_coin_ids
    }
    return diagnostics


def _wait_until_inputs_spent(
    *,
    coinset: CoinsetAdapter,
    input_coin_ids: list[str],
    timeout_seconds: int,
    poll_seconds: int,
    warning_interval_seconds: int,
    sleep_fn: Any = time.sleep,
    monotonic_fn: Any = time.monotonic,
) -> dict[str, Any]:
    start = monotonic_fn()
    next_warning = max(1, int(warning_interval_seconds))
    poll_count = 0
    warnings: list[dict[str, Any]] = []
    last_pending: list[str] = list(input_coin_ids)
    while True:
        poll_count += 1
        elapsed = int(monotonic_fn() - start)
        try:
            records_by_id = _coinset_records_by_name(
                coinset=coinset,
                coin_ids=input_coin_ids,
                include_spent_coins=True,
            )
        except Exception as exc:  # noqa: BLE001
            warnings.append(
                {
                    "event": "verify_poll_error",
                    "elapsed_seconds": elapsed,
                    "error": str(exc),
                }
            )
            if elapsed >= int(timeout_seconds):
                return {
                    "status": "timeout",
                    "poll_count": poll_count,
                    "elapsed_seconds": elapsed,
                    "pending_coin_ids": last_pending,
                    "warnings": warnings,
                }
            sleep_fn(max(1, int(poll_seconds)))
            continue
        pending: list[str] = []
        spent_heights: dict[str, int] = {}
        for coin_id in input_coin_ids:
            record = records_by_id.get(coin_id)
            spent_height = _coin_spent_height_from_record(record) if isinstance(record, dict) else 0
            spent_heights[coin_id] = spent_height
            if spent_height <= 0:
                pending.append(coin_id)
        if not pending:
            return {
                "status": "spent",
                "poll_count": poll_count,
                "elapsed_seconds": elapsed,
                "spent_heights": spent_heights,
                "warnings": warnings,
            }
        last_pending = pending
        if elapsed >= int(timeout_seconds):
            return {
                "status": "timeout",
                "poll_count": poll_count,
                "elapsed_seconds": elapsed,
                "pending_coin_ids": pending,
                "warnings": warnings,
            }
        if elapsed >= next_warning:
            warnings.append(
                {
                    "event": "verify_wait_warning",
                    "elapsed_seconds": elapsed,
                    "pending_coin_count": len(pending),
                }
            )
            next_warning += max(1, int(warning_interval_seconds))
        sleep_fn(max(1, int(poll_seconds)))
    # Unreachable; keep return for type checkers.
    return {"status": "timeout", "pending_coin_ids": last_pending}


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Direct CAT combine from explicit Coinset coin names: "
            "build one-output spend, sign via KMS vault context, push via Coinset, verify spent."
        )
    )
    parser.add_argument(
        "--coin-name", action="append", default=[], help="Coin name hex (repeat or CSV)."
    )
    parser.add_argument("--max-input-coins", type=int, default=10)
    parser.add_argument(
        "--stepwise-combine",
        action="store_true",
        help=(
            "Allow multiple sequential combine submissions when more than --max-input-coins "
            "are provided. Each chunk is submitted and verified separately."
        ),
    )
    parser.add_argument(
        "--allow-sub-cat-output",
        action="store_true",
        help=(
            "Override CAT floor guard for this script only. Allows intermediate combines "
            "that may output <1000 mojos (useful for stepwise cleanup of many tiny inputs)."
        ),
    )
    parser.add_argument("--preflight-only", action="store_true")
    parser.add_argument(
        "--debug-broadcast-diagnostics",
        action="store_true",
        help=(
            "On broadcast failure, rebuild bundle and include tx/mempool/coin-state diagnostics "
            "to compare with Cloud Wallet combine behavior."
        ),
    )
    parser.add_argument("--kms-live-probe", action="store_true")
    parser.add_argument(
        "--kms-live-probe-message-hex",
        default="00" * 32,
        help="Hex message for optional live KMS sign probe.",
    )
    parser.add_argument("--network", default="mainnet", choices=["mainnet", "testnet11", "testnet"])
    parser.add_argument("--coinset-base-url", default="")
    parser.add_argument("--key-id", default="")
    parser.add_argument("--keyring-yaml-path", default="")
    parser.add_argument("--receive-address", default="")
    parser.add_argument("--cloud-wallet-base-url", default="")
    parser.add_argument("--cloud-wallet-user-key-id", default="")
    parser.add_argument("--cloud-wallet-private-key-pem-path", default="")
    parser.add_argument("--vault-id", default="")
    parser.add_argument("--cloud-wallet-kms-key-id", default="")
    parser.add_argument("--cloud-wallet-kms-region", default="us-west-2")
    parser.add_argument("--cloud-wallet-kms-public-key-hex", default="")
    parser.add_argument(
        "--cloud-wallet-vault-nonce-probe-max",
        type=int,
        default=2048,
        help="Maximum singleton nonce to probe for vault CAT p2 hash matching (default 2048).",
    )
    parser.add_argument("--verify-timeout-seconds", type=int, default=15 * 60)
    parser.add_argument("--verify-poll-seconds", type=int, default=8)
    parser.add_argument("--verify-warning-interval-seconds", type=int, default=5 * 60)
    parser.add_argument(
        "--cat-resolution-max-attempts",
        type=int,
        default=4,
        help="Retry attempts for CAT resolution from selected input coin IDs.",
    )
    parser.add_argument(
        "--cat-resolution-retry-sleep-seconds",
        type=float,
        default=1.0,
        help="Sleep between CAT-resolution retry attempts.",
    )
    return parser


def _validate_required_args(args: argparse.Namespace) -> list[str]:
    missing: list[str] = []
    required = {
        "--key-id": args.key_id,
        "--keyring-yaml-path": args.keyring_yaml_path,
        "--receive-address": args.receive_address,
        "--cloud-wallet-base-url": args.cloud_wallet_base_url,
        "--cloud-wallet-user-key-id": args.cloud_wallet_user_key_id,
        "--cloud-wallet-private-key-pem-path": args.cloud_wallet_private_key_pem_path,
        "--vault-id": args.vault_id,
        "--cloud-wallet-kms-key-id": args.cloud_wallet_kms_key_id,
    }
    for flag, value in required.items():
        if not str(value).strip():
            missing.append(flag)
    return missing


def run(
    args: argparse.Namespace,
    *,
    coinset_factory: Any = CoinsetAdapter,
    cloud_wallet_factory: Any = CloudWalletAdapter,
    sign_and_broadcast_fn: Any = sign_and_broadcast_mixed_split,
    kms_pubkey_resolver: Any = get_public_key_compressed_hex,
    kms_signer: Any = sign_digest,
    sleep_fn: Any = time.sleep,
    monotonic_fn: Any = time.monotonic,
) -> tuple[int, dict[str, Any]]:
    missing_flags = _validate_required_args(args)
    coin_names_raw = _parse_csv_values(list(args.coin_name))
    normalized_coin_names = _normalize_coin_names(coin_names_raw)
    max_input_coins = max(2, int(args.max_input_coins))
    selected_coin_ids = _select_input_coin_ids(normalized_coin_names, max_input_coins)
    payload: dict[str, Any] = {
        "network": str(args.network).strip(),
        "requested_coin_count": len(normalized_coin_names),
        "selected_input_coin_ids": selected_coin_ids,
        "max_input_coins": max_input_coins,
        "stepwise_combine": bool(args.stepwise_combine),
        "allow_sub_cat_output": bool(args.allow_sub_cat_output),
    }
    if missing_flags:
        payload["status"] = "error"
        payload["reason"] = "missing_required_flags"
        payload["missing_flags"] = missing_flags
        return 1, payload
    if len(normalized_coin_names) < 2:
        payload["status"] = "error"
        payload["reason"] = "at_least_two_coin_names_required"
        return 1, payload
    if len(selected_coin_ids) < 2:
        payload["status"] = "error"
        payload["reason"] = "at_least_two_selected_inputs_required"
        return 1, payload
    if len(normalized_coin_names) > max_input_coins and not bool(args.stepwise_combine):
        payload["status"] = "error"
        payload["reason"] = "input_count_exceeds_single_spendbundle_limit"
        payload["operator_guidance"] = (
            "pass --stepwise-combine to run sequential combine chunks, or provide fewer --coin-name values"
        )
        return 1, payload

    require_testnet11 = str(args.network).strip().lower() in {"testnet", "testnet11"}
    coinset = coinset_factory(
        base_url=_normalize_coinset_base_url(base_url=args.coinset_base_url, network=args.network),
        network=args.network,
        require_testnet11=require_testnet11,
    )

    execution_coin_ids = (
        list(normalized_coin_names) if bool(args.stepwise_combine) else list(selected_coin_ids)
    )
    resolved_inputs, input_issues = _resolve_input_coins(
        coinset=coinset, coin_ids=execution_coin_ids
    )
    total_amount = sum(int(coin.amount) for coin in resolved_inputs)
    min_inputs_for_floor = _min_coin_count_for_target_mojos(
        amounts=[int(coin.amount) for coin in resolved_inputs],
        target_mojos=_MIN_CAT_OUTPUT_MOJOS,
    )
    asset_id, asset_check = _resolve_cat_asset_id_for_coin_ids(
        network=coinset.network,
        coin_ids=[coin.coin_id for coin in resolved_inputs],
        max_attempts=max(1, int(args.cat_resolution_max_attempts)),
        retry_sleep_seconds=max(0.0, float(args.cat_resolution_retry_sleep_seconds)),
        sleep_fn=sleep_fn,
    )
    preflight = _preflight_checks(
        args=args,
        coinset=coinset,
        selected_coin_ids=[coin.coin_id for coin in resolved_inputs],
        total_amount=total_amount,
        asset_id=asset_id,
        cloud_wallet_factory=cloud_wallet_factory,
        kms_pubkey_resolver=kms_pubkey_resolver,
        kms_signer=kms_signer,
    )

    payload["coinset_base_url"] = coinset.base_url
    payload["input_resolution"] = {
        "resolved_count": len(resolved_inputs),
        "issues": input_issues,
        "resolved_inputs": [
            {"coin_id": coin.coin_id, "amount": coin.amount} for coin in resolved_inputs
        ],
        "minimum_inputs_needed_for_cat_floor": min_inputs_for_floor,
    }
    payload["asset_resolution"] = asset_check
    payload["preflight"] = preflight

    if bool(args.preflight_only):
        payload["status"] = "preflight_ok" if preflight.get("ready", False) else "preflight_failed"
        return (0 if preflight.get("ready", False) else 1), payload

    if input_issues:
        payload["status"] = "error"
        payload["reason"] = "input_validation_failed"
        return 1, payload
    if not preflight.get("ready", False):
        payload["status"] = "error"
        payload["reason"] = "preflight_not_ready"
        return 1, payload
    if not asset_id:
        payload["status"] = "error"
        payload["reason"] = "asset_resolution_failed"
        return 1, payload
    if total_amount <= 0:
        payload["status"] = "error"
        payload["reason"] = "invalid_total_amount"
        return 1, payload
    if total_amount < _MIN_CAT_OUTPUT_MOJOS and not bool(args.allow_sub_cat_output):
        payload["status"] = "error"
        payload["reason"] = "cat_total_below_minimum_mojos"
        payload["minimum_mojos"] = int(_MIN_CAT_OUTPUT_MOJOS)
        payload["total_amount"] = int(total_amount)
        payload["operator_guidance"] = (
            "select more/larger inputs, or pass --allow-sub-cat-output (combine script override only) "
            "for intermediate stepwise combines"
        )
        return 1, payload
    if not bool(args.stepwise_combine):
        signing_payload = _build_signing_payload(
            args=args,
            selected_coin_ids=[coin.coin_id for coin in resolved_inputs],
            asset_id=asset_id,
            output_amount=total_amount,
        )
        payload["signing_plan"] = {
            "asset_id": asset_id,
            "input_coin_count": len(resolved_inputs),
            "output_amount": total_amount,
            "fee_mojos": 0,
        }
        broadcast = sign_and_broadcast_fn(signing_payload)
        payload["broadcast"] = broadcast
        if str(broadcast.get("status", "")).strip().lower() != "executed":
            if bool(args.debug_broadcast_diagnostics):
                payload["broadcast_diagnostics"] = _build_broadcast_diagnostics(
                    signing_payload=signing_payload,
                    coinset=coinset,
                    input_coin_ids=[coin.coin_id for coin in resolved_inputs],
                )
            payload["status"] = "error"
            payload["reason"] = f"broadcast_failed:{broadcast.get('reason', 'unknown')}"
            return 1, payload

        verification = _wait_until_inputs_spent(
            coinset=coinset,
            input_coin_ids=[coin.coin_id for coin in resolved_inputs],
            timeout_seconds=max(1, int(args.verify_timeout_seconds)),
            poll_seconds=max(1, int(args.verify_poll_seconds)),
            warning_interval_seconds=max(1, int(args.verify_warning_interval_seconds)),
            sleep_fn=sleep_fn,
            monotonic_fn=monotonic_fn,
        )
        payload["verification"] = verification
        if str(verification.get("status", "")).strip().lower() != "spent":
            payload["status"] = "error"
            payload["reason"] = "verification_timeout"
            return 1, payload
        payload["status"] = "ok"
        return 0, payload

    # Stepwise mode: split inputs into <= max_input_coins chunks and combine each chunk.
    stepwise_chunks = _partition_stepwise_chunks(
        [coin.coin_id for coin in resolved_inputs], max_input_coins=max_input_coins
    )
    stepwise_chunk_coin_ids = {coin_id for chunk in stepwise_chunks for coin_id in chunk}
    stepwise_leftovers = [
        coin.coin_id for coin in resolved_inputs if coin.coin_id not in stepwise_chunk_coin_ids
    ]
    if stepwise_leftovers:
        payload["stepwise_leftover_coin_ids"] = stepwise_leftovers
        payload["stepwise_leftover_count"] = len(stepwise_leftovers)
    if not stepwise_chunks:
        payload["status"] = "error"
        payload["reason"] = "stepwise_no_valid_chunks"
        return 1, payload
    step_results: list[dict[str, Any]] = []
    for chunk_index, chunk_coin_ids in enumerate(stepwise_chunks, start=1):
        chunk_resolved, chunk_issues = _resolve_input_coins(
            coinset=coinset, coin_ids=chunk_coin_ids
        )
        chunk_total = sum(int(coin.amount) for coin in chunk_resolved)
        if chunk_issues:
            payload["status"] = "error"
            payload["reason"] = "stepwise_input_validation_failed"
            payload["stepwise_results"] = step_results
            payload["failed_chunk"] = {
                "index": chunk_index,
                "chunk_coin_ids": chunk_coin_ids,
                "issues": chunk_issues,
            }
            return 1, payload
        if chunk_total < _MIN_CAT_OUTPUT_MOJOS and not bool(args.allow_sub_cat_output):
            payload["status"] = "error"
            payload["reason"] = "stepwise_chunk_total_below_minimum_mojos"
            payload["minimum_mojos"] = int(_MIN_CAT_OUTPUT_MOJOS)
            payload["stepwise_results"] = step_results
            payload["failed_chunk"] = {
                "index": chunk_index,
                "chunk_coin_ids": chunk_coin_ids,
                "chunk_total_mojos": int(chunk_total),
                "operator_guidance": (
                    "pass --allow-sub-cat-output to permit intermediate dust outputs in stepwise mode"
                ),
            }
            return 1, payload
        signing_payload = _build_signing_payload(
            args=args,
            selected_coin_ids=[coin.coin_id for coin in chunk_resolved],
            asset_id=asset_id,
            output_amount=chunk_total,
        )
        broadcast = sign_and_broadcast_fn(signing_payload)
        chunk_payload: dict[str, Any] = {
            "index": chunk_index,
            "input_coin_count": len(chunk_resolved),
            "input_coin_ids": [coin.coin_id for coin in chunk_resolved],
            "output_amount": int(chunk_total),
            "broadcast": broadcast,
        }
        if str(broadcast.get("status", "")).strip().lower() != "executed":
            if bool(args.debug_broadcast_diagnostics):
                chunk_payload["broadcast_diagnostics"] = _build_broadcast_diagnostics(
                    signing_payload=signing_payload,
                    coinset=coinset,
                    input_coin_ids=[coin.coin_id for coin in chunk_resolved],
                )
            step_results.append(chunk_payload)
            payload["status"] = "error"
            payload["reason"] = f"stepwise_broadcast_failed:{broadcast.get('reason', 'unknown')}"
            payload["stepwise_results"] = step_results
            return 1, payload
        verification = _wait_until_inputs_spent(
            coinset=coinset,
            input_coin_ids=[coin.coin_id for coin in chunk_resolved],
            timeout_seconds=max(1, int(args.verify_timeout_seconds)),
            poll_seconds=max(1, int(args.verify_poll_seconds)),
            warning_interval_seconds=max(1, int(args.verify_warning_interval_seconds)),
            sleep_fn=sleep_fn,
            monotonic_fn=monotonic_fn,
        )
        chunk_payload["verification"] = verification
        step_results.append(chunk_payload)
        if str(verification.get("status", "")).strip().lower() != "spent":
            payload["status"] = "error"
            payload["reason"] = "stepwise_verification_timeout"
            payload["stepwise_results"] = step_results
            return 1, payload

    payload["status"] = "ok"
    payload["stepwise_results"] = step_results
    payload["stepwise_chunk_count"] = len(step_results)
    return 0, payload


def main(argv: list[str] | None = None) -> int:
    parser = _build_parser()
    args = parser.parse_args(argv)
    exit_code, payload = run(args)
    print(json.dumps(payload, indent=2))
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
