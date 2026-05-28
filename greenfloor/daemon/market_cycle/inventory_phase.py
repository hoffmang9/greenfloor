"""Market cycle inventory scan phase."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.config.models import ProgramConfig, signer_offer_path_configured
from greenfloor.core.cycle import (
    needs_inventory_fallback,
    resolve_inventory_scan_source,
    should_try_cat_inventory_fallback,
)
from greenfloor.core.inventory import compute_bucket_counts_from_coins
from greenfloor.daemon.inventory_scan import (
    _coinset_cat_spendable_base_unit_coin_amounts,
    _coinset_spendable_base_unit_coin_amounts,
)
from greenfloor.daemon.market_helpers import _base_unit_mojo_multiplier_for_market
from greenfloor.daemon.market_logging import _daemon_logger, _log_market_decision
from greenfloor.daemon.strategy_dispatch import _resolve_signer_offer_asset_ids_for_reservation
from greenfloor.storage.sqlite import SqliteStore


def run_market_cycle_inventory(
    *,
    market: Any,
    program: Any,
    wallet: WalletAdapter,
    store: SqliteStore,
    sell_ladder: list[Any],
) -> dict[int, int] | None:
    ladder_sizes = [e.size_base_units for e in sell_ladder]
    bucket_counts: dict[int, int] | None = None
    wallet_coins: list[int] = []
    coinset_scan_empty = False
    coinset_scan_found_coins = False
    cat_scan_found_coins = False
    wallet_scan_found_coins = False

    if isinstance(program, ProgramConfig) and signer_offer_path_configured(program):
        try:
            resolved_base_asset_id, _, _ = _resolve_signer_offer_asset_ids_for_reservation(
                program=program,
                market=market,
            )
            wallet_coins = _coinset_spendable_base_unit_coin_amounts(
                program=program,
                market=market,
                resolved_asset_id=resolved_base_asset_id,
                base_unit_mojo_multiplier=_base_unit_mojo_multiplier_for_market(market=market),
            )
            coinset_scan_empty = len(wallet_coins) == 0
            if wallet_coins:
                coinset_scan_found_coins = True
                bucket_counts = compute_bucket_counts_from_coins(
                    coin_amounts_base_units=wallet_coins,
                    ladder_sizes=ladder_sizes,
                )
                _log_market_decision(
                    market.market_id,
                    "inventory_scan_wallet",
                    source="coinset",
                    resolved_asset_id=resolved_base_asset_id,
                    coin_count=len(wallet_coins),
                    bucket_counts=bucket_counts,
                )
                store.add_audit_event(
                    "inventory_bucket_scan",
                    {
                        "market_id": market.market_id,
                        "source": "coinset",
                        "resolved_asset_id": resolved_base_asset_id,
                        "bucket_counts": bucket_counts,
                        "coin_count": len(wallet_coins),
                    },
                    market_id=market.market_id,
                )
        except Exception as exc:
            _daemon_logger.warning(
                "coinset_inventory_scan_failed market_id=%s error=%s",
                market.market_id,
                exc,
            )

    if needs_inventory_fallback(
        bucket_counts_available=bucket_counts is not None,
        coinset_scan_empty=coinset_scan_empty,
    ):
        wallet_coins = []
        if should_try_cat_inventory_fallback(
            coinset_scan_empty=coinset_scan_empty,
            base_asset=str(market.base_asset),
        ):
            wallet_coins = _coinset_cat_spendable_base_unit_coin_amounts(
                canonical_asset_id=str(market.base_asset),
                receive_address=str(market.receive_address),
                network=str(program.app_network),
                base_unit_mojo_multiplier=_base_unit_mojo_multiplier_for_market(market=market),
            )
            cat_scan_found_coins = len(wallet_coins) > 0
        if not wallet_coins:
            wallet_coins = wallet.list_asset_coins_base_units(
                asset_id=market.base_asset,
                key_id=market.signer_key_id,
                receive_address=market.receive_address,
                network=program.app_network,
            )
            wallet_scan_found_coins = len(wallet_coins) > 0
        if wallet_coins:
            bucket_counts = compute_bucket_counts_from_coins(
                coin_amounts_base_units=wallet_coins,
                ladder_sizes=ladder_sizes,
            )
            fallback_source = resolve_inventory_scan_source(
                coinset_scan_found_coins=coinset_scan_found_coins,
                coinset_scan_empty=coinset_scan_empty,
                cat_scan_found_coins=cat_scan_found_coins,
                wallet_scan_found_coins=wallet_scan_found_coins,
            )
            _log_market_decision(
                market.market_id,
                "inventory_scan_wallet",
                source=fallback_source,
                coin_count=len(wallet_coins),
                bucket_counts=bucket_counts,
            )
            store.add_audit_event(
                "inventory_bucket_scan",
                {
                    "market_id": market.market_id,
                    "source": fallback_source,
                    "bucket_counts": bucket_counts,
                    "coin_count": len(wallet_coins),
                },
                market_id=market.market_id,
            )
        else:
            bucket_counts = dict(market.inventory.bucket_counts)
            fallback_source = resolve_inventory_scan_source(
                coinset_scan_found_coins=coinset_scan_found_coins,
                coinset_scan_empty=coinset_scan_empty,
                cat_scan_found_coins=cat_scan_found_coins,
                wallet_scan_found_coins=wallet_scan_found_coins,
            )
            _log_market_decision(
                market.market_id,
                "inventory_scan_config_fallback",
                asset_id=market.base_asset,
                bucket_counts=bucket_counts,
                source=fallback_source,
            )
            store.add_audit_event(
                "inventory_bucket_scan",
                {
                    "market_id": market.market_id,
                    "source": fallback_source,
                    "asset_id": market.base_asset,
                    "bucket_counts": bucket_counts,
                },
                market_id=market.market_id,
            )
    return bucket_counts
