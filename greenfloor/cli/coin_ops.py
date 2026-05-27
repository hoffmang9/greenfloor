"""CLI Cloud Wallet coin listing and split/combine operations."""

from __future__ import annotations

import logging
import math
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig
from greenfloor.asset_label_catalog import _is_hex_asset_id
from greenfloor.cli.offer_build_post import resolve_market_for_build
from greenfloor.cli.prompts import prompt_yes_no
from greenfloor.config.io import load_markets_config_with_optional_overlay, load_program_config
from greenfloor.core.coin_ops_policy import coin_meets_coin_op_min_amount, coin_op_min_amount_mojos
from greenfloor.runtime import coinset_runtime
from greenfloor.runtime.cloud_wallet import adapter as cloud_wallet_adapter
from greenfloor.runtime.cloud_wallet import assets as cloud_wallet_assets
from greenfloor.runtime.cloud_wallet import polling as cloud_wallet_polling
from greenfloor.runtime.cloud_wallet.adapter import (
    _format_json_output as format_json_output,
)
from greenfloor.runtime.cloud_wallet.adapter import (
    _require_cloud_wallet_config as require_cloud_wallet_config,
)
from greenfloor.runtime.cloud_wallet.coins import (
    classify_resolved_coin_ids_by_asset,
    coin_asset_id,
    coin_matches_direct_spendable_lookup,
    is_spendable_coin,
    resolve_coin_global_ids,
)
from greenfloor.runtime.coinset_runtime import CoinsetFeeLookupPreflightError

coin_ops_logger = logging.getLogger("greenfloor.manager")


def resolve_cloud_wallet_asset_id_for_wallet(
    *,
    wallet: CloudWalletAdapter,
    canonical_asset_id: str,
    symbol_hint: str | None = None,
    allow_dexie_lookup: bool = True,
    program_home_dir: str | None = None,
) -> str:
    return cloud_wallet_assets.resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=canonical_asset_id,
        symbol_hint=symbol_hint,
        allow_dexie_lookup=allow_dexie_lookup,
        program_home_dir=program_home_dir,
    )


def wallet_with_optional_vault_override(
    program,
    *,
    vault_id: str | None,
) -> cloud_wallet_adapter.CloudWalletAdapter:
    wallet = cloud_wallet_adapter.new_cloud_wallet_adapter(program)
    if vault_id and vault_id.strip() and vault_id.strip() != wallet.vault_id:
        override_config = require_cloud_wallet_config(program)
        wallet = cloud_wallet_adapter.CloudWalletAdapter(
            CloudWalletConfig(
                base_url=override_config.base_url,
                user_key_id=override_config.user_key_id,
                private_key_pem_path=override_config.private_key_pem_path,
                vault_id=vault_id.strip(),
                network=override_config.network,
            )
        )
    return wallet


def wallet_asset_amounts_for_scope(
    *,
    wallet: CloudWalletAdapter,
    asset_id: str,
) -> tuple[int | None, int | None, int | None]:
    """Return (total, spendable, locked) amounts for a resolved wallet asset id."""
    if not hasattr(wallet, "_graphql"):
        return None, None, None
    query = """
query walletAssetAmounts($walletId: ID!, $first: Int) {
  wallet(id: $walletId) {
    assets(first: $first) {
      edges {
        node {
          assetId
          totalAmount
          spendableAmount
          lockedAmount
        }
      }
    }
  }
}
"""
    try:
        payload = wallet._graphql(
            query=query,
            variables={"walletId": wallet.vault_id, "first": 100},
        )
    except Exception:
        return None, None, None
    wallet_payload = payload.get("wallet") or {}
    assets_payload = wallet_payload.get("assets") or {}
    edges = assets_payload.get("edges") or []
    target = asset_id.strip()
    for edge in edges:
        node = edge.get("node") if isinstance(edge, dict) else None
        if not isinstance(node, dict):
            continue
        node_asset_id = str(node.get("assetId", "")).strip()
        if node_asset_id != target:
            continue
        try:
            total_amount = int(node.get("totalAmount", 0))
            spendable_amount = int(node.get("spendableAmount", 0))
            locked_amount = int(node.get("lockedAmount", 0))
        except (TypeError, ValueError):
            return None, None, None
        return total_amount, spendable_amount, locked_amount
    return None, None, None


def evaluate_denomination_readiness(
    *,
    wallet: CloudWalletAdapter,
    asset_id: str,
    size_base_units: int,
    required_min_count: int | None = None,
    max_allowed_count: int | None = None,
) -> dict[str, int | bool | str]:
    coins = wallet.list_coins(include_pending=True)
    spendable = [
        c
        for c in coins
        if is_spendable_coin(c)
        and coin_asset_id(c).lower() == asset_id.strip().lower()
        and int(c.get("amount", 0)) == int(size_base_units)
    ]
    current_count = len(spendable)
    ready = True
    if required_min_count is not None:
        ready = current_count >= int(required_min_count)
    if max_allowed_count is not None:
        ready = ready and current_count <= int(max_allowed_count)
    return {
        "asset_id": asset_id,
        "size_base_units": int(size_base_units),
        "current_count": current_count,
        "required_min_count": int(required_min_count) if required_min_count is not None else -1,
        "max_allowed_count": int(max_allowed_count) if max_allowed_count is not None else -1,
        "ready": ready,
    }


def as_wait_events(value: object) -> list[dict[str, str]]:
    if not isinstance(value, list):
        return []
    items: list[dict[str, str]] = []
    for row in value:
        if isinstance(row, dict):
            event = {str(k): str(v) for k, v in row.items()}
            items.append(event)
    return items


# ---------------------------------------------------------------------------
# Shared coin-operation helpers
# ---------------------------------------------------------------------------


def coin_op_base_payload(
    market: Any, selected_venue: str | None, wallet: CloudWalletAdapter
) -> dict[str, object]:
    return {
        "market_id": market.market_id,
        "pair": f"{market.base_symbol}:{market.quote_asset}",
        "venue": selected_venue,
        "vault_id": wallet.vault_id,
    }


def resolve_coin_op_fee(
    *,
    network: str,
    minimum_fee_mojos: int,
    market: Any,
    selected_venue: str | None,
    wallet: CloudWalletAdapter,
) -> tuple[int, str] | None:
    """Resolve fee for a coin operation.

    Returns ``(fee_mojos, fee_source)`` on success or ``None`` after printing
    a structured JSON error payload.
    """
    try:
        return coinset_runtime._resolve_taker_or_coin_operation_fee(
            network=network,
            minimum_fee_mojos=minimum_fee_mojos,
        )
    except CoinsetFeeLookupPreflightError as exc:
        operator_guidance = (
            "verify Coinset endpoint routing: unset GREENFLOOR_COINSET_BASE_URL to use "
            "network defaults, or set it to a valid endpoint for the active network"
            if exc.failure_kind == "endpoint_validation_failed"
            else "coinset fee advice is temporarily unavailable; retry shortly and verify Coinset fee endpoint health before resubmitting"
        )
        print(
            format_json_output(
                {
                    **coin_op_base_payload(market, selected_venue, wallet),
                    "waited": False,
                    "success": False,
                    "error": f"coinset_fee_preflight_failed:{exc.failure_kind}",
                    "coinset_fee_lookup": {
                        "status": "failed",
                        "failure_kind": exc.failure_kind,
                        "detail": exc.detail,
                        **exc.diagnostics,
                    },
                    "operator_guidance": operator_guidance,
                }
            )
        )
        return None
    except Exception as exc:
        print(
            format_json_output(
                {
                    **coin_op_base_payload(market, selected_venue, wallet),
                    "waited": False,
                    "success": False,
                    "error": f"fee_resolution_failed:{exc}",
                    "operator_guidance": (
                        "set coin_ops.minimum_fee_mojos in program config (can be 0) "
                        "or fix GREENFLOOR_COINSET_BASE_URL to a valid Coinset API endpoint"
                    ),
                }
            )
        )
        return None


def effective_coin_split_fee_for_asset(
    *,
    canonical_asset_id: str,
    resolved_asset_id: str,
    fee_mojos: int,
    fee_source: str,
) -> tuple[int, str]:
    """Return coin-split fee policy for the target asset."""
    _ = resolved_asset_id
    return int(fee_mojos), str(fee_source)


def coin_op_build_iteration_payload(
    *,
    wallet: CloudWalletAdapter,
    signature_request_id: str,
    initial_signature_state: str,
    no_wait: bool,
    network: str,
    existing_coin_ids: set[str],
    iteration: int,
    denomination_target: dict[str, Any] | None,
    readiness_asset_id: str,
    readiness_kwargs: dict[str, int],
) -> tuple[dict[str, object], dict[str, int | bool | str] | None]:
    """Poll signature, wait for confirmation, evaluate readiness."""
    wait_events: list[dict[str, str]] = []
    final_signature_state = initial_signature_state
    if not no_wait:
        final_signature_state, signature_events = (
            cloud_wallet_polling.poll_signature_request_until_not_unsigned(
                wallet=wallet,
                signature_request_id=signature_request_id,
                timeout_seconds=15 * 60,
                warning_interval_seconds=10 * 60,
            )
        )
        wait_events.extend(signature_events)
        wait_events.extend(
            cloud_wallet_polling.wait_for_mempool_then_confirmation(
                wallet=wallet,
                network=network,
                initial_coin_ids=existing_coin_ids,
                include_pending=True,
                mempool_warning_seconds=5 * 60,
                confirmation_warning_seconds=15 * 60,
            )
        )
    iteration_payload: dict[str, object] = {
        "iteration": iteration,
        "signature_request_id": signature_request_id,
        "signature_state": final_signature_state,
        "waited": not no_wait,
        "wait_events": wait_events,
    }
    final_readiness = None
    if denomination_target is not None:
        final_readiness = evaluate_denomination_readiness(
            wallet=wallet,
            asset_id=readiness_asset_id,
            size_base_units=int(denomination_target["size_base_units"]),
            **readiness_kwargs,
        )
        iteration_payload["denomination_readiness"] = final_readiness
    return iteration_payload, final_readiness


def coin_op_should_stop(
    *,
    until_ready: bool,
    final_readiness: dict[str, int | bool | str] | None,
    coin_ids: list[str],
    iteration: int,
    max_iterations: int,
) -> tuple[bool, str]:
    """Return ``(should_break, stop_reason)`` for the iteration loop."""
    if not until_ready or final_readiness is None or bool(final_readiness["ready"]):
        stop_reason = "ready" if until_ready and final_readiness is not None else "single_pass"
        return True, stop_reason
    if coin_ids:
        return True, "requires_new_coin_selection"
    if iteration == max_iterations:
        return True, "max_iterations_reached"
    return False, ""


def coin_op_unresolved_error(
    *,
    market: Any,
    selected_venue: str | None,
    wallet: CloudWalletAdapter,
    unresolved_coin_ids: list[str],
) -> str:
    return format_json_output(
        {
            **coin_op_base_payload(market, selected_venue, wallet),
            "waited": False,
            "success": False,
            "error": "coin_id_resolution_failed",
            "unknown_coin_ids": unresolved_coin_ids,
            "operator_guidance": (
                "run greenfloor-manager coins-list and pass coin_id values from output; "
                "manager accepts hex coin names and resolves them to Cloud Wallet Coin_* ids"
            ),
        }
    )


def coin_split_lockup_guardrail_error(
    *,
    market: Any,
    selected_venue: str | None,
    wallet: CloudWalletAdapter,
    resolved_asset_id: str,
    spendable_asset_coin_ids: set[str],
    selected_coin_ids: list[str],
) -> str:
    selected_spendable_ids = sorted(set(selected_coin_ids) & spendable_asset_coin_ids)
    return format_json_output(
        {
            **coin_op_base_payload(market, selected_venue, wallet),
            "waited": False,
            "success": False,
            "error": "coin_split_guardrail_would_lock_all_spendable_coins",
            "resolved_asset_id": resolved_asset_id,
            "spendable_asset_coin_count": len(spendable_asset_coin_ids),
            "selected_spendable_coin_count": len(selected_spendable_ids),
            "selected_spendable_coin_ids": selected_spendable_ids,
            "operator_guidance": (
                "coin-split would consume all currently spendable coins for this asset; "
                "leave at least one spendable coin free or pass --allow-lock-all-spendable "
                "to override intentionally"
            ),
        }
    )


def evaluate_coin_split_gate(
    *,
    asset_scoped_coins: list[dict],
    resolved_asset_id: str,
    size_base_units: int,
    required_count: int,
) -> dict[str, int | bool | str]:
    spendable_asset_coins = [coin for coin in asset_scoped_coins if is_spendable_coin(coin)]
    denom_coins = [
        coin for coin in spendable_asset_coins if int(coin.get("amount", 0)) == int(size_base_units)
    ]
    larger_reserve_coins = [
        coin for coin in spendable_asset_coins if int(coin.get("amount", 0)) > int(size_base_units)
    ]
    current_count = len(denom_coins)
    extra_denom_count = max(0, current_count - int(required_count))
    larger_reserve_count = len(larger_reserve_coins)
    reserve_ready = larger_reserve_count >= 1 or extra_denom_count >= 1
    ready = current_count >= int(required_count) and reserve_ready
    return {
        "asset_id": resolved_asset_id,
        "size_base_units": int(size_base_units),
        "required_min_count": int(required_count),
        "current_count": current_count,
        "larger_reserve_coin_count": larger_reserve_count,
        "extra_denom_coin_count": extra_denom_count,
        "reserve_ready": reserve_ready,
        "ready": ready,
    }


def coin_op_result_payload(
    *,
    market: Any,
    selected_venue: str | None,
    wallet: CloudWalletAdapter,
    coin_ids: list[str],
    denomination_target: dict[str, Any] | None,
    until_ready: bool,
    max_iterations: int,
    stop_reason: str,
    final_readiness: dict[str, int | bool | str] | None,
    operations: list[dict[str, object]],
    fee_mojos: int,
    fee_source: str,
) -> dict[str, object]:
    return {
        **coin_op_base_payload(market, selected_venue, wallet),
        "coin_selection_mode": "explicit" if coin_ids else "adapter_auto_select",
        "denomination_target": denomination_target,
        "until_ready": until_ready,
        "max_iterations": max_iterations,
        "stop_reason": stop_reason,
        "denomination_readiness": final_readiness,
        "operations": operations,
        "signature_request_id": (
            str(operations[-1].get("signature_request_id", "")) if operations else ""
        ),
        "signature_state": (
            str(operations[-1].get("signature_state", "UNKNOWN")) if operations else "UNKNOWN"
        ),
        "waited": bool(operations[-1].get("waited", False)) if operations else False,
        "wait_events": (
            as_wait_events(operations[-1].get("wait_events", [])) if operations else []
        ),
        "fee_mojos": fee_mojos,
        "fee_source": fee_source,
    }


def resolve_venue_for_coin_prep(*, venue_override: str | None) -> str | None:
    if venue_override is None or not venue_override.strip():
        return None
    venue = venue_override.strip().lower()
    if venue not in {"dexie", "splash"}:
        raise ValueError("coin-prep venue must be dexie or splash when provided")
    return venue


def resolve_market_denomination_entry(market, *, size_base_units: int):
    ladder = market.ladders.get("sell") or []
    if not ladder:
        raise ValueError(
            f"market {market.market_id} has no sell ladder; cannot resolve denomination target"
        )
    for entry in ladder:
        if int(entry.size_base_units) == int(size_base_units):
            return entry
    allowed = ", ".join(str(int(row.size_base_units)) for row in ladder)
    raise ValueError(
        f"size_base_units not configured for market sell ladder; use one of: {allowed}"
    )


def coins_list(
    *,
    program_path: Path,
    asset: str | None,
    vault_id: str | None,
    cat_id: str | None = None,
) -> int:
    program = load_program_config(program_path)
    wallet = wallet_with_optional_vault_override(program, vault_id=vault_id)

    resolved_asset_filter: str | None = None
    if cat_id and cat_id.strip():
        raw_cat_id = cat_id.strip().lower()
        if not _is_hex_asset_id(raw_cat_id):
            raise ValueError("--cat-id must be a 64-character hex CAT asset id")
        resolved_asset_filter = resolve_cloud_wallet_asset_id_for_wallet(
            wallet=wallet,
            canonical_asset_id=raw_cat_id,
            symbol_hint=None,
            allow_dexie_lookup=False,
            program_home_dir=str(program.home_dir),
        )
    elif asset and asset.strip():
        effective_asset = asset.strip()
        resolved_asset_filter = resolve_cloud_wallet_asset_id_for_wallet(
            wallet=wallet,
            canonical_asset_id=effective_asset,
            symbol_hint=effective_asset,
            program_home_dir=str(program.home_dir),
        )
    coins = wallet.list_coins(asset_id=resolved_asset_filter, include_pending=True)
    filtered_asset_id = str(resolved_asset_filter or "").strip().lower()
    scoped_asset_id = str(resolved_asset_filter).strip() if filtered_asset_id else None
    items = []
    for coin in coins:
        coin_state = str(coin.get("state", "")).strip().upper()
        pending = coin_state in {"PENDING", "MEMPOOL"}
        spendable = is_spendable_coin(coin)
        asset_raw = coin.get("asset")
        # Asset-scoped queries now intentionally omit `coin.asset` because the
        # upstream resolver can report a bogus fallback asset. Preserve missing
        # row metadata as `None`; only concrete conflicting ids should trigger
        # the mixed-asset warning path below.
        reported_asset_id: str | None = None
        if isinstance(asset_raw, dict):
            raw_reported_asset_id = str(asset_raw.get("id", "")).strip()
            reported_asset_id = raw_reported_asset_id or None
        # When Cloud Wallet coin listing is asset-scoped, trust the query scope for
        # membership and normalize output asset id to that scope. Some backends
        # may return mixed asset metadata in scoped responses.
        output_asset_id = scoped_asset_id if filtered_asset_id else (reported_asset_id or "xch")
        items.append(
            {
                "coin_id": str(coin.get("name", coin.get("id", ""))).strip(),
                "amount": int(coin.get("amount", 0)),
                "state": coin_state or "UNKNOWN",
                "pending": pending,
                "spendable": spendable,
                "asset": output_asset_id,
                "reported_asset": reported_asset_id,
                "scoped_asset": scoped_asset_id,
            }
        )
    scoped_total_amount: int | None = None
    scoped_spendable_amount: int | None = None
    scoped_locked_amount: int | None = None
    if filtered_asset_id:
        (
            scoped_total_amount,
            scoped_spendable_amount,
            scoped_locked_amount,
        ) = wallet_asset_amounts_for_scope(
            wallet=wallet,
            asset_id=str(resolved_asset_filter).strip(),
        )
    warnings: list[dict[str, Any]] = []
    items_amount_sum = sum(int(item.get("amount", 0)) for item in items)
    raw_scoped_total_amount = scoped_total_amount
    asset_totals_withheld_reason: str | None = None
    if filtered_asset_id:
        # Ignore missing row-level asset metadata here; the scoped query may omit
        # it on purpose as a workaround for the upstream fallback-to-XCH bug.
        distinct_reported_asset_ids = sorted(
            {
                reported_asset_id.strip()
                for item in items
                for reported_asset_id in [item.get("reported_asset")]
                if isinstance(reported_asset_id, str) and reported_asset_id.strip()
            }
        )
        unexpected_reported_asset_ids = sorted(
            {
                reported_asset_id
                for reported_asset_id in distinct_reported_asset_ids
                if reported_asset_id.lower() != filtered_asset_id
            }
        )
        if unexpected_reported_asset_ids:
            warning_payload = {
                "code": "mixed_reported_asset_ids_detected",
                "message": "asset-scoped coin query returned mixed reported asset ids; scoped asset totals withheld",
                "resolved_asset_id": scoped_asset_id,
                "reported_asset_ids": distinct_reported_asset_ids,
                "unexpected_reported_asset_ids": unexpected_reported_asset_ids,
            }
            warnings.append(warning_payload)
            coin_ops_logger.warning(
                "coins_list_mixed_asset_metadata vault_id=%s resolved_asset_id=%s reported_asset_ids=%s",
                wallet.vault_id,
                scoped_asset_id,
                ",".join(distinct_reported_asset_ids),
            )
            asset_totals_withheld_reason = "mixed_reported_asset_ids_detected"
            scoped_total_amount = None
            scoped_spendable_amount = None
            scoped_locked_amount = None
    if raw_scoped_total_amount is not None and items_amount_sum != int(raw_scoped_total_amount):
        warning_payload = {
            "code": "item_amount_sum_mismatch",
            "message": "sum(items.amount) does not match wallet asset total amount",
            "resolved_asset_id": scoped_asset_id,
            "items_amount_sum": items_amount_sum,
            "wallet_asset_total_amount": int(raw_scoped_total_amount),
            "difference_amount": items_amount_sum - int(raw_scoped_total_amount),
        }
        warnings.append(warning_payload)
        coin_ops_logger.warning(
            "coins_list_amount_mismatch vault_id=%s resolved_asset_id=%s items_amount_sum=%s wallet_asset_total_amount=%s difference_amount=%s",
            wallet.vault_id,
            scoped_asset_id,
            items_amount_sum,
            int(raw_scoped_total_amount),
            items_amount_sum - int(raw_scoped_total_amount),
        )
    print(
        format_json_output(
            {
                "vault_id": wallet.vault_id,
                "network": wallet.network,
                "resolved_asset_id": scoped_asset_id,
                "count": len(items),
                "item_amount_sum": items_amount_sum,
                "items": items,
                "asset_total_amount": scoped_total_amount,
                "asset_spendable_amount": scoped_spendable_amount,
                "asset_locked_amount": scoped_locked_amount,
                "asset_totals_withheld_reason": asset_totals_withheld_reason,
                "warnings": warnings,
            }
        )
    )
    return 0


def seed_wallet_assets_cache_cli(
    *,
    program_path: Path,
    vault_id: str | None,
) -> int:
    program = load_program_config(program_path)
    wallet = wallet_with_optional_vault_override(program, vault_id=vault_id)
    try:
        payload = cloud_wallet_assets.seed_cloud_wallet_assets_cache(
            wallet=wallet,
            program_home_dir=str(program.home_dir),
        )
    except Exception as exc:
        print(format_json_output({"ok": False, "error": str(exc)}), file=sys.stderr)
        return 1
    print(format_json_output({"ok": True, **payload}))
    return 0


def coin_status(
    *,
    program_path: Path,
    asset: str | None,
    vault_id: str | None,
    cat_id: str | None = None,
) -> int:
    """Show per-coin state/spendability for an optional asset scope."""
    return coins_list(
        program_path=program_path,
        asset=asset,
        vault_id=vault_id,
        cat_id=cat_id,
    )


@dataclass(slots=True)
class CoinOpSetup:
    program: Any
    market: Any
    wallet: CloudWalletAdapter
    resolved_asset_id: str
    fee_mojos: int
    fee_source: str
    selected_venue: str | None


def coin_op_setup(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    network: str,
    market_id: str | None,
    pair: str | None,
    venue: str | None,
    canonical_asset_id_override: str | None = None,
) -> CoinOpSetup | None:
    program = load_program_config(program_path)
    selected_venue = resolve_venue_for_coin_prep(venue_override=venue)
    markets = load_markets_config_with_optional_overlay(
        path=markets_path,
        overlay_path=testnet_markets_path,
    )
    market = resolve_market_for_build(
        markets,
        market_id=market_id,
        pair=pair,
        network=network,
    )
    wallet = cloud_wallet_adapter.new_cloud_wallet_adapter(program)
    canonical = canonical_asset_id_override or str(market.base_asset)
    hint = canonical_asset_id_override or str(market.base_symbol)
    resolved_asset_id = resolve_cloud_wallet_asset_id_for_wallet(
        wallet=wallet,
        canonical_asset_id=canonical,
        symbol_hint=hint,
        program_home_dir=str(program.home_dir),
    )
    fee_result = resolve_coin_op_fee(
        network=network,
        minimum_fee_mojos=int(program.coin_ops_minimum_fee_mojos),
        market=market,
        selected_venue=selected_venue,
        wallet=wallet,
    )
    if fee_result is None:
        return None
    fee_mojos, fee_source = fee_result
    return CoinOpSetup(
        program=program,
        market=market,
        wallet=wallet,
        resolved_asset_id=resolved_asset_id,
        fee_mojos=fee_mojos,
        fee_source=fee_source,
        selected_venue=selected_venue,
    )


def coin_split(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    network: str,
    market_id: str | None,
    pair: str | None,
    coin_ids: list[str],
    amount_per_coin: int,
    number_of_coins: int,
    no_wait: bool,
    venue: str | None = None,
    size_base_units: int | None = None,
    until_ready: bool = False,
    max_iterations: int = 3,
    allow_lock_all_spendable: bool = False,
    force_split_when_ready: bool = False,
    prompt_for_override: bool | None = None,
) -> int:
    if until_ready and no_wait:
        raise ValueError("until-ready mode requires wait mode (do not pass --no-wait)")
    if until_ready and size_base_units is None:
        raise ValueError("until-ready mode requires --size-base-units")
    if max_iterations <= 0:
        raise ValueError("max_iterations must be positive")
    setup = coin_op_setup(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        network=network,
        market_id=market_id,
        pair=pair,
        venue=venue,
    )
    if setup is None:
        return 2
    market = setup.market
    wallet = setup.wallet
    resolved_split_asset_id = setup.resolved_asset_id
    fee_mojos, fee_source = effective_coin_split_fee_for_asset(
        canonical_asset_id=str(market.base_asset),
        resolved_asset_id=str(setup.resolved_asset_id),
        fee_mojos=setup.fee_mojos,
        fee_source=setup.fee_source,
    )
    selected_venue = setup.selected_venue
    denomination_target = None
    if size_base_units is not None and int(size_base_units) > 0:
        entry = resolve_market_denomination_entry(market, size_base_units=int(size_base_units))
        required_count = int(entry.target_count) + int(entry.split_buffer_count)
        if amount_per_coin <= 0:
            amount_per_coin = int(entry.size_base_units)
        elif amount_per_coin != int(entry.size_base_units):
            raise ValueError(
                "amount_per_coin must match market ladder size when --size-base-units is set"
            )
        if number_of_coins <= 0:
            number_of_coins = required_count
        elif number_of_coins != required_count:
            raise ValueError(
                "number_of_coins must match market ladder target+buffer when --size-base-units is set"
            )
        denomination_target = {
            "size_base_units": int(entry.size_base_units),
            "target_count": int(entry.target_count),
            "split_buffer_count": int(entry.split_buffer_count),
            "required_count": required_count,
        }
    min_coin_amount_mojos = coin_op_min_amount_mojos(canonical_asset_id=str(market.base_asset))
    if amount_per_coin <= 0:
        raise ValueError("amount_per_coin must be positive")
    if number_of_coins <= 0:
        raise ValueError("number_of_coins must be positive")

    operations: list[dict[str, object]] = []
    final_readiness: dict[str, int | bool | str] | None = None
    split_gate: dict[str, int | bool | str] | None = None
    stop_reason = "single_pass"
    unresolved_coin_ids: list[str] = []

    for iteration in range(1, max_iterations + 1):
        wallet_coins = wallet.list_coins(include_pending=True)
        existing_coin_ids = {str(c.get("id", "")).strip() for c in wallet_coins}
        asset_scoped_coins = wallet.list_coins(
            asset_id=resolved_split_asset_id,
            include_pending=True,
        )
        spendable_asset_coin_ids = {
            str(c.get("id", "")).strip()
            for c in asset_scoped_coins
            if is_spendable_coin(c)
            and coin_meets_coin_op_min_amount(c, canonical_asset_id=str(market.base_asset))
            and str(c.get("id", "")).strip()
        }
        if denomination_target is not None:
            split_gate = evaluate_coin_split_gate(
                asset_scoped_coins=asset_scoped_coins,
                resolved_asset_id=resolved_split_asset_id,
                size_base_units=int(denomination_target["size_base_units"]),
                required_count=int(denomination_target["required_count"]),
            )
            final_readiness = split_gate
            if bool(split_gate["ready"]) and not force_split_when_ready:
                if prompt_yes_no(
                    (
                        "split gate is already satisfied "
                        "(target+buffer met and reserve available). Force another split anyway?"
                    ),
                    prompt_for_override=prompt_for_override,
                ):
                    pass
                else:
                    stop_reason = "ready"
                    break
        if coin_ids:
            resolved_coin_ids, unresolved_coin_ids = resolve_coin_global_ids(wallet_coins, coin_ids)
            if unresolved_coin_ids:
                break
        else:
            spendable_asset_coins = [
                c
                for c in asset_scoped_coins
                if is_spendable_coin(c)
                and coin_meets_coin_op_min_amount(c, canonical_asset_id=str(market.base_asset))
            ]
            if not spendable_asset_coins:
                print(
                    format_json_output(
                        {
                            **coin_op_base_payload(market, selected_venue, wallet),
                            "waited": False,
                            "success": False,
                            "error": "no_spendable_split_coin_available",
                            "asset_id": str(market.base_asset),
                            "resolved_asset_id": resolved_split_asset_id,
                            "temporary_min_coin_amount_mojos": int(min_coin_amount_mojos),
                            "operator_guidance": (
                                "no spendable coins are currently available for this asset; "
                                "wait for pending/signature requests to settle or free locked offers, "
                                "then retry coin-split. Temporary workaround: CAT split selection "
                                "ignores coins smaller than 1 CAT unit (1000 mojos)."
                            ),
                        }
                    )
                )
                return 2
            selected_coin = max(
                spendable_asset_coins,
                key=lambda coin: int(coin.get("amount", 0)),
            )
            selected_coin_global_id = str(selected_coin.get("id", "")).strip()
            if not selected_coin_global_id:
                raise RuntimeError("coin_split_failed:missing_selected_coin_id")
            resolved_coin_ids = [selected_coin_global_id]
            unresolved_coin_ids = []

        if (
            not allow_lock_all_spendable
            and spendable_asset_coin_ids
            and set(resolved_coin_ids) >= spendable_asset_coin_ids
        ):
            if prompt_yes_no(
                (
                    "coin-split would lock all currently spendable coins for this asset. "
                    "Override and continue?"
                ),
                prompt_for_override=prompt_for_override,
            ):
                pass
            else:
                print(
                    coin_split_lockup_guardrail_error(
                        market=market,
                        selected_venue=selected_venue,
                        wallet=wallet,
                        resolved_asset_id=resolved_split_asset_id,
                        spendable_asset_coin_ids=spendable_asset_coin_ids,
                        selected_coin_ids=resolved_coin_ids,
                    )
                )
                return 2

        split_result = wallet.split_coins(
            coin_ids=resolved_coin_ids,
            amount_per_coin=amount_per_coin,
            number_of_coins=number_of_coins,
            fee=fee_mojos,
        )
        signature_request_id = split_result["signature_request_id"]
        if not signature_request_id:
            raise RuntimeError("coin_split_failed:missing_signature_request_id")

        readiness_kwargs: dict[str, int] = {}
        if denomination_target is not None:
            readiness_kwargs["required_min_count"] = int(denomination_target["required_count"])
        iteration_payload, final_readiness = coin_op_build_iteration_payload(
            wallet=wallet,
            signature_request_id=signature_request_id,
            initial_signature_state=split_result.get("status", "UNKNOWN"),
            no_wait=no_wait,
            network=network,
            existing_coin_ids=existing_coin_ids,
            iteration=iteration,
            denomination_target=denomination_target,
            readiness_asset_id=str(market.base_asset),
            readiness_kwargs=readiness_kwargs,
        )
        operations.append(iteration_payload)

        should_break, reason = coin_op_should_stop(
            until_ready=until_ready,
            final_readiness=final_readiness,
            coin_ids=coin_ids,
            iteration=iteration,
            max_iterations=max_iterations,
        )
        if should_break:
            stop_reason = reason
            break

    if unresolved_coin_ids:
        print(
            coin_op_unresolved_error(
                market=market,
                selected_venue=selected_venue,
                wallet=wallet,
                unresolved_coin_ids=unresolved_coin_ids,
            )
        )
        return 2
    print(
        format_json_output(
            {
                **coin_op_result_payload(
                    market=market,
                    selected_venue=selected_venue,
                    wallet=wallet,
                    coin_ids=coin_ids,
                    denomination_target=denomination_target,
                    until_ready=until_ready,
                    max_iterations=max_iterations,
                    stop_reason=stop_reason,
                    final_readiness=final_readiness,
                    operations=operations,
                    fee_mojos=fee_mojos,
                    fee_source=fee_source,
                ),
                "amount_per_coin": amount_per_coin,
                "number_of_coins": number_of_coins,
                "resolved_asset_id": resolved_split_asset_id,
                "split_gate": split_gate,
            }
        )
    )
    if until_ready and final_readiness is not None and not bool(final_readiness["ready"]):
        return 2
    return 0


def coin_combine(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    network: str,
    market_id: str | None,
    pair: str | None,
    number_of_coins: int,
    asset_id: str | None,
    coin_ids: list[str],
    no_wait: bool,
    venue: str | None = None,
    size_base_units: int | None = None,
    until_ready: bool = False,
    max_iterations: int = 3,
) -> int:
    if until_ready and no_wait:
        raise ValueError("until-ready mode requires wait mode (do not pass --no-wait)")
    if until_ready and size_base_units is None:
        raise ValueError("until-ready mode requires --size-base-units")
    if max_iterations <= 0:
        raise ValueError("max_iterations must be positive")
    requested_asset_id = asset_id.strip() if asset_id else None
    setup = coin_op_setup(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        network=network,
        market_id=market_id,
        pair=pair,
        venue=venue,
        canonical_asset_id_override=requested_asset_id,
    )
    if setup is None:
        return 2
    market = setup.market
    wallet = setup.wallet
    resolved_asset_id = setup.resolved_asset_id
    fee_mojos = setup.fee_mojos
    fee_source = setup.fee_source
    selected_venue = setup.selected_venue
    denomination_target = None
    if size_base_units is not None and int(size_base_units) > 0:
        entry = resolve_market_denomination_entry(market, size_base_units=int(size_base_units))
        threshold = max(
            2,
            int(math.ceil(int(entry.target_count) * float(entry.combine_when_excess_factor))),
        )
        if number_of_coins <= 0:
            number_of_coins = threshold
        elif number_of_coins != threshold:
            raise ValueError(
                "number_of_coins must match market ladder combine threshold when --size-base-units is set"
            )
        denomination_target = {
            "size_base_units": int(entry.size_base_units),
            "target_count": int(entry.target_count),
            "combine_when_excess_factor": float(entry.combine_when_excess_factor),
            "combine_threshold_count": threshold,
        }
    if number_of_coins <= 1:
        raise ValueError("number_of_coins must be > 1")

    operations: list[dict[str, object]] = []
    final_readiness: dict[str, int | bool | str] | None = None
    stop_reason = "single_pass"
    unresolved_coin_ids: list[str] = []
    combine_canonical_asset_id = requested_asset_id or str(market.base_asset)
    min_coin_amount_mojos = coin_op_min_amount_mojos(canonical_asset_id=combine_canonical_asset_id)

    for iteration in range(1, max_iterations + 1):
        wallet_coins = wallet.list_coins(include_pending=True)
        existing_coin_ids = {str(c.get("id", "")).strip() for c in wallet_coins}
        resolved_input_coin_ids: list[str] | None = None
        if coin_ids:
            resolved_input_coin_ids, unresolved_coin_ids = resolve_coin_global_ids(
                wallet_coins, coin_ids
            )
            if unresolved_coin_ids:
                break
            if number_of_coins != len(resolved_input_coin_ids):
                raise ValueError(
                    "when --coin-id is provided, --input-coin-count must match the number of --coin-id values"
                )
            unresolved_coin_ids, mismatched_coin_ids = classify_resolved_coin_ids_by_asset(
                wallet_coins=wallet_coins,
                resolved_coin_ids=resolved_input_coin_ids,
                expected_asset_id=resolved_asset_id,
            )
            if unresolved_coin_ids:
                break
            if mismatched_coin_ids:
                print(
                    format_json_output(
                        {
                            **coin_op_base_payload(market, selected_venue, wallet),
                            "waited": False,
                            "success": False,
                            "error": "coin_id_asset_mismatch",
                            "resolved_asset_id": resolved_asset_id,
                            "mismatched_coin_ids": [
                                str(entry.get("coin_id", "")).strip()
                                for entry in mismatched_coin_ids
                                if str(entry.get("coin_id", "")).strip()
                            ],
                            "mismatched_coin_assets": mismatched_coin_ids,
                            "operator_guidance": (
                                "all explicit --coin-id values must resolve to the same asset "
                                "as --asset-id; re-run coins-list scoped to the target asset "
                                "and retry with only those coin ids"
                            ),
                        }
                    )
                )
                return 2
        elif min_coin_amount_mojos > 0:
            asset_scoped_coins = wallet.list_coins(asset_id=resolved_asset_id, include_pending=True)
            direct_lookup_cache: dict[str, bool] = {}
            eligible_asset_coins = [
                c
                for c in asset_scoped_coins
                if is_spendable_coin(c)
                and coin_meets_coin_op_min_amount(c, canonical_asset_id=combine_canonical_asset_id)
                and coin_matches_direct_spendable_lookup(
                    wallet=wallet,
                    coin=c,
                    scoped_asset_id=resolved_asset_id,
                    cache=direct_lookup_cache,
                )
                and str(c.get("id", "")).strip()
            ]
            if len(eligible_asset_coins) < number_of_coins:
                print(
                    format_json_output(
                        {
                            **coin_op_base_payload(market, selected_venue, wallet),
                            "waited": False,
                            "success": False,
                            "error": "insufficient_combine_coins_after_temp_cat_floor",
                            "asset_id": combine_canonical_asset_id,
                            "resolved_asset_id": resolved_asset_id,
                            "required_coin_count": int(number_of_coins),
                            "eligible_coin_count": len(eligible_asset_coins),
                            "temporary_min_coin_amount_mojos": int(min_coin_amount_mojos),
                            "operator_guidance": (
                                "not enough spendable coins remain after ignoring CAT coins "
                                "smaller than 1 CAT unit (1000 mojos). Wait for larger coins, "
                                "re-split inventory, or pass explicit --coin-id values if you "
                                "intend to override the temporary workaround."
                            ),
                        }
                    )
                )
                return 2
            eligible_asset_coins.sort(key=lambda coin: int(coin.get("amount", 0)), reverse=True)
            resolved_input_coin_ids = [
                str(coin.get("id", "")).strip() for coin in eligible_asset_coins[:number_of_coins]
            ]

        combine_result = wallet.combine_coins(
            number_of_coins=number_of_coins,
            fee=fee_mojos,
            asset_id=resolved_asset_id,
            largest_first=True,
            input_coin_ids=resolved_input_coin_ids,
        )
        signature_request_id = combine_result["signature_request_id"]
        if not signature_request_id:
            raise RuntimeError("coin_combine_failed:missing_signature_request_id")

        readiness_kwargs: dict[str, int] = {}
        if denomination_target is not None:
            readiness_kwargs["max_allowed_count"] = int(
                denomination_target["combine_threshold_count"]
            )
        iteration_payload, final_readiness = coin_op_build_iteration_payload(
            wallet=wallet,
            signature_request_id=signature_request_id,
            initial_signature_state=combine_result.get("status", "UNKNOWN"),
            no_wait=no_wait,
            network=network,
            existing_coin_ids=existing_coin_ids,
            iteration=iteration,
            denomination_target=denomination_target,
            readiness_asset_id=resolved_asset_id,
            readiness_kwargs=readiness_kwargs,
        )
        operations.append(iteration_payload)

        should_break, reason = coin_op_should_stop(
            until_ready=until_ready,
            final_readiness=final_readiness,
            coin_ids=coin_ids,
            iteration=iteration,
            max_iterations=max_iterations,
        )
        if should_break:
            stop_reason = reason
            break

    if unresolved_coin_ids:
        print(
            coin_op_unresolved_error(
                market=market,
                selected_venue=selected_venue,
                wallet=wallet,
                unresolved_coin_ids=unresolved_coin_ids,
            )
        )
        return 2
    print(
        format_json_output(
            {
                **coin_op_result_payload(
                    market=market,
                    selected_venue=selected_venue,
                    wallet=wallet,
                    coin_ids=coin_ids,
                    denomination_target=denomination_target,
                    until_ready=until_ready,
                    max_iterations=max_iterations,
                    stop_reason=stop_reason,
                    final_readiness=final_readiness,
                    operations=operations,
                    fee_mojos=fee_mojos,
                    fee_source=fee_source,
                ),
                "asset_id": requested_asset_id or str(market.base_asset).strip(),
                "resolved_asset_id": resolved_asset_id,
                "number_of_coins": number_of_coins,
            }
        )
    )
    if until_ready and final_readiness is not None and not bool(final_readiness["ready"]):
        return 2
    return 0
