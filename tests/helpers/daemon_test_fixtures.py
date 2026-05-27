"""Shared fixtures for daemon unit tests."""

from __future__ import annotations

import threading
from dataclasses import dataclass, replace
from datetime import UTC, datetime, timedelta
from pathlib import Path
from types import SimpleNamespace
from typing import Any, cast

import pytest

from greenfloor.config import io as config_io
from greenfloor.config.models import (
    MarketConfig,
    MarketInventoryConfig,
    ProgramConfig,
    VaultConfig,
    VaultWalletKeyConfig,
)
from greenfloor.core.strategy import PlannedAction
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.testing import (
    POST_COOLDOWN_UNTIL,
    PENDING_VISIBILITY_REASON,
    MarketCycleResult,
    MarketDispatchState,
    ReservationContentionError,
    ReservationStorageError,
    cooldown_remaining_ms,
    active_offer_counts_by_size,
    active_offer_counts_by_size_and_side,
    build_dexie_size_by_offer_id,
    build_offer_for_action,
    coinset_spendable_base_unit_coin_amounts,
    detect_stale_open_offers_for_requeue,
    drop_zero_repeat_strategy_actions,
    enqueue_immediate_requeue_market,
    execute_single_local_action,
    execute_strategy_actions,
    expand_strategy_actions,
    inject_reseed_action_if_no_active_offers,
    inventory_scan,
    match_watched_coin_ids,
    reconcile_market_cycle_offers,
    resolve_quote_asset_for_offer,
    select_market_batch,
    set_watched_coin_ids_for_market,
    single_input_preferred_skip_reason,
    strategy_config_from_market,
    strategy_dispatch,
    update_market_coin_watchlist_from_dexie,
)
from greenfloor.runtime.coin_ops.planning import select_spendable_coins_for_target_amount
from greenfloor.storage.sqlite import SqliteStore
from tests.helpers.config_fixtures import minimal_program_config


def _execute_local_strategy_actions(
    *,
    market: MarketConfig,
    strategy_actions: list[PlannedAction],
    program: ProgramConfig,
    xch_price_usd: float | None,
    dexie: Any,
    store: Any,
    splash: Any | None = None,
    publish_venue: str = "dexie",
    keyring_yaml_path: str = "",
    **_: Any,
) -> dict[str, Any]:
    expanded = expand_strategy_actions(strategy_actions)
    items: list[dict[str, Any]] = []
    executed_count = 0
    for action in expanded:
        item = execute_single_local_action(
            program=program,
            market=market,
            action=action,
            xch_price_usd=xch_price_usd,
            keyring_yaml_path=keyring_yaml_path,
            dexie=dexie,
            splash=splash,
            publish_venue=publish_venue,
            store=store,
        )
        if item.get("status") == "executed":
            executed_count += 1
        items.append(item)
    return {
        "planned_count": len(expanded),
        "executed_count": executed_count,
        "items": items,
    }


def _signer_program_config(**overrides: Any) -> ProgramConfig:
    vault = VaultConfig(
        launcher_id="0" * 64,
        custody_threshold=1,
        recovery_threshold=1,
        recovery_clawback_timelock=3600,
        custody_keys=(VaultWalletKeyConfig(public_key_hex="02" + "00" * 31, curve="SECP256R1"),),
        recovery_keys=(),
    )
    base = minimal_program_config()
    return replace(
        base,
        signer_kms_key_id="kms-key",
        vault_config=vault,
        runtime_offer_parallelism_enabled=bool(
            overrides.get("runtime_offer_parallelism_enabled", False)
        ),
        runtime_offer_parallelism_max_workers=int(
            overrides.get("runtime_offer_parallelism_max_workers", 2)
        ),
        runtime_reservation_ttl_seconds=int(overrides.get("runtime_reservation_ttl_seconds", 300)),
    )


class _FakeDexie:
    def __init__(self, post_result: dict):
        self.post_result = post_result
        self.posted: list[str] = []
        self.calls = 0
        self.visible_offer_ids: set[str] = set()

    def post_offer(self, offer: str) -> dict:
        self.posted.append(offer)
        self.calls += 1
        return dict(self.post_result)

    def get_offer(self, offer_id: str) -> dict[str, Any]:
        clean_offer_id = str(offer_id).strip()
        if clean_offer_id in self.visible_offer_ids:
            return {"success": True, "offer": {"id": clean_offer_id, "status": 0}}
        raise RuntimeError("dexie_http_error:404")


class _FakeStore:
    def __init__(self) -> None:
        self.offer_states: list[dict] = []
        self.audit_events: list[dict] = []

    def upsert_offer_state(
        self, *, offer_id: str, market_id: str, state: str, last_seen_status: int | None
    ) -> None:
        self.offer_states.append(
            {
                "offer_id": offer_id,
                "market_id": market_id,
                "state": state,
                "last_seen_status": last_seen_status,
            }
        )

    def list_offer_states(self, *, market_id: str | None = None, limit: int = 200) -> list[dict]:
        _ = market_id, limit
        return list(self.offer_states)

    def list_recent_audit_events(
        self,
        *,
        event_types: list[str] | None = None,
        market_id: str | None = None,
        limit: int = 50,
    ) -> list[dict]:
        rows = list(self.audit_events)
        if event_types:
            allowed = set(event_types)
            rows = [row for row in rows if str(row.get("event_type", "")) in allowed]
        if market_id:
            rows = [row for row in rows if str(row.get("market_id", "")) == market_id]
        return rows[: int(limit)]

    def add_audit_event(self, event_type: str, payload: dict, market_id: str | None = None) -> None:
        self.audit_events.insert(
            0,
            {
                "event_type": str(event_type),
                "market_id": market_id,
                "payload": dict(payload),
            },
        )


def _coin_ops_base_unit_mojo_multiplier(market: Any) -> int:
    pricing = getattr(market, "pricing", None)
    if isinstance(pricing, dict):
        return int(pricing.get("base_unit_mojo_multiplier", 1000))
    return int(getattr(pricing, "base_unit_mojo_multiplier", 1000))


class _CoinOpsProgram:
    """Minimal program stub for coin-op tests (includes dry-run and fee fields)."""

    runtime_dry_run = False
    app_network = "mainnet"
    signer_kms_key_id = "kms-key"
    vault_config = SimpleNamespace(launcher_id="0" * 64)
    coin_ops_split_fee_mojos = 0
    coin_ops_combine_fee_mojos = 0
    home_dir = "/tmp/greenfloor-test"


def _market() -> MarketConfig:
    return MarketConfig(
        market_id="m1",
        enabled=True,
        base_asset="asset",
        base_symbol="BYC",
        quote_asset="xch",
        quote_asset_type="unstable",
        receive_address="xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        mode="sell_only",
        signer_key_id="key-main-1",
        inventory=MarketInventoryConfig(low_watermark_base_units=100),
        pricing={
            "fixed_quote_per_base": 0.5,
            "base_unit_mojo_multiplier": 1000,
            "quote_unit_mojo_multiplier": 1000,
        },
    )


__all__ = [
    "Any",
    "AssetReservationCoordinator",
    "MarketConfig",
    "MarketCycleResult",
    "MarketDispatchState",
    "Path",
    "PlannedAction",
    "POST_COOLDOWN_UNTIL",
    "PENDING_VISIBILITY_REASON",
    "ProgramConfig",
    "ReservationContentionError",
    "ReservationStorageError",
    "SimpleNamespace",
    "SqliteStore",
    "UTC",
    "cast",
    "config_io",
    "cooldown_remaining_ms",
    "dataclass",
    "datetime",
    "replace",
    "strategy_dispatch",
    "threading",
    "timedelta",
    "_CoinOpsProgram",
    "_FakeDexie",
    "_FakeStore",
    "_coin_ops_base_unit_mojo_multiplier",
    "_execute_local_strategy_actions",
    "_market",
    "_signer_program_config",
    "active_offer_counts_by_size",
    "active_offer_counts_by_size_and_side",
    "build_dexie_size_by_offer_id",
    "build_offer_for_action",
    "coinset_spendable_base_unit_coin_amounts",
    "detect_stale_open_offers_for_requeue",
    "drop_zero_repeat_strategy_actions",
    "enqueue_immediate_requeue_market",
    "execute_strategy_actions",
    "expand_strategy_actions",
    "inject_reseed_action_if_no_active_offers",
    "inventory_scan",
    "match_watched_coin_ids",
    "minimal_program_config",
    "pytest",
    "reconcile_market_cycle_offers",
    "resolve_quote_asset_for_offer",
    "select_market_batch",
    "select_spendable_coins_for_target_amount",
    "set_watched_coin_ids_for_market",
    "single_input_preferred_skip_reason",
    "strategy_config_from_market",
    "update_market_coin_watchlist_from_dexie",
]
