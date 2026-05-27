"""CLI Cloud Wallet coin listing commands."""

from __future__ import annotations

import logging
import sys
from pathlib import Path
from typing import Any

from greenfloor.asset_label_catalog import _is_hex_asset_id
from greenfloor.config.io import load_markets_config, load_program_config
from greenfloor.config.models import ProgramConfig, coin_ops_execution_backend
from greenfloor.runtime.cloud_wallet import assets as cloud_wallet_assets
from greenfloor.runtime.cloud_wallet.adapter import format_json_output
from greenfloor.runtime.cloud_wallet.coin_ops_runtime import wallet_with_optional_vault_override
from greenfloor.runtime.cloud_wallet.coins import is_spendable_coin
from greenfloor.runtime.coin_ops_backend import build_coin_op_backend, resolve_signer_asset_id

coin_ops_logger = logging.getLogger("greenfloor.manager")


def _resolve_coins_list_market(program: ProgramConfig) -> Any:
    markets_path = Path(program.home_dir) / "config" / "markets.yaml"
    markets = load_markets_config(markets_path)
    enabled = [market for market in markets.markets if market.enabled]
    candidates = enabled or list(markets.markets)
    if not candidates:
        raise ValueError("no markets configured")
    if len(candidates) == 1:
        return candidates[0]
    return min(candidates, key=lambda market: str(market.market_id))


def _coins_list_signer(
    *,
    program: Any,
    asset: str | None,
    cat_id: str | None,
) -> int:
    assert isinstance(program, ProgramConfig)
    market = _resolve_coins_list_market(program)
    receive_address = str(market.receive_address).strip()
    if not receive_address:
        raise ValueError("market missing receive_address for signer coin list")

    canonical_filter = None
    if cat_id and cat_id.strip():
        canonical_filter = cat_id.strip().lower()
    elif asset and asset.strip():
        canonical_filter = asset.strip()
    resolved_asset_filter: str | None = None
    if canonical_filter:
        resolved_asset_filter = resolve_signer_asset_id(
            program, canonical_asset_id=canonical_filter, symbol_hint=canonical_filter
        )
    list_asset_id = resolved_asset_filter or str(market.base_asset)
    backend = build_coin_op_backend(
        program=program,
        market=market,
        selected_venue=None,
        resolved_asset_id=list_asset_id,
    )
    coins = backend.list_asset_scoped_coins()
    items = []
    for coin in coins:
        coin_state = str(coin.get("state", "CONFIRMED")).strip().upper()
        items.append(
            {
                "coin_id": str(coin.get("name", coin.get("id", ""))).strip(),
                "amount": int(coin.get("amount", 0)),
                "state": coin_state,
                "pending": coin_state in {"PENDING", "MEMPOOL"},
                "spendable": is_spendable_coin(coin),
                "asset": resolved_asset_filter or list_asset_id,
                "reported_asset": resolved_asset_filter,
                "scoped_asset": resolved_asset_filter,
            }
        )
    spendable = [c for c in items if bool(c.get("spendable"))]
    print(
        format_json_output(
            {
                "execution_backend": "signer",
                "market_id": market.market_id,
                "receive_address": receive_address,
                "resolved_asset_id": resolved_asset_filter,
                "coins": items,
                "coin_count": len(items),
                "spendable_coin_count": len(spendable),
                "spendable_amount": sum(int(c["amount"]) for c in spendable),
            }
        )
    )
    return 0


def coins_list(
    *,
    program_path: Path,
    asset: str | None,
    vault_id: str | None,
    cat_id: str | None = None,
) -> int:
    program = load_program_config(program_path)
    if coin_ops_execution_backend(program) == "signer":
        return _coins_list_signer(program=program, asset=asset, cat_id=cat_id)
    wallet = wallet_with_optional_vault_override(program, vault_id=vault_id)

    resolved_asset_filter: str | None = None
    if cat_id and cat_id.strip():
        raw_cat_id = cat_id.strip().lower()
        if not _is_hex_asset_id(raw_cat_id):
            raise ValueError("--cat-id must be a 64-character hex CAT asset id")
        resolved_asset_filter = cloud_wallet_assets.resolve_cloud_wallet_asset_id(
            wallet=wallet,
            canonical_asset_id=raw_cat_id,
            symbol_hint=None,
            allow_dexie_lookup=False,
            program_home_dir=str(program.home_dir),
        )
    elif asset and asset.strip():
        effective_asset = asset.strip()
        resolved_asset_filter = cloud_wallet_assets.resolve_cloud_wallet_asset_id(
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
        reported_asset_id: str | None = None
        if isinstance(asset_raw, dict):
            raw_reported_asset_id = str(asset_raw.get("id", "")).strip()
            reported_asset_id = raw_reported_asset_id or None
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
        ) = cloud_wallet_assets.wallet_asset_amounts_for_scope(
            wallet=wallet,
            asset_id=str(resolved_asset_filter).strip(),
        )
    warnings: list[dict[str, Any]] = []
    items_amount_sum = sum(int(item.get("amount", 0)) for item in items)
    raw_scoped_total_amount = scoped_total_amount
    asset_totals_withheld_reason: str | None = None
    if filtered_asset_id:
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
