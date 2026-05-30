"""Stable public hooks for daemon unit and integration tests.

Production orchestration should import implementation modules directly. Tests should
prefer imports from this package so refactors do not require wide private-symbol churn.

Canonical monkeypatch targets (module-qualified paths):

- ``greenfloor.daemon.testing.main`` — cycle orchestration (`run_once`, `run_loop`, adapters; aliases ``cycle_runner``)
- ``greenfloor.daemon.main`` — CLI entrypoint and instance lock
- ``greenfloor.daemon.testing.inventory_scan`` — coinset adapter factory
- ``greenfloor.daemon.testing.strategy_state`` — reseed policy
- ``greenfloor.daemon.testing.cooldowns`` — post/cancel cooldown globals
- ``greenfloor.daemon.testing.cancel_policy`` — cancel policy entry
"""

from __future__ import annotations

from greenfloor.core.cycle import expand_planned_actions, single_input_preferred_skip_reason
from greenfloor.daemon.testing.cancel_policy import execute_cancel_policy
from greenfloor.daemon.testing.cooldowns import (
    CANCEL_COOLDOWN_UNTIL,
    POST_COOLDOWN_UNTIL,
    cancel_retry_config,
    cooldown_remaining_ms,
    cooldowns,
    post_retry_config,
    set_cooldown,
)
from greenfloor.daemon.testing.helpers import PlannedAction, drop_zero_repeat_strategy_actions
from greenfloor.daemon.testing.inventory_scan import (
    CoinsetAdapter,
    coinset_spendable_base_unit_coin_amounts,
    inventory_scan,
)
from greenfloor.daemon.testing.main import (
    MarketCycleResult,
    MarketDispatchState,
    consume_reload_marker,
    main,
    run_loop,
    run_once,
)
from greenfloor.daemon.testing.market_helpers import resolve_quote_asset_for_offer
from greenfloor.daemon.testing.reconcile import reconcile_market_cycle_offers
from greenfloor.daemon.testing.reservations import (
    AssetReservationCoordinator,
    ReservationContentionError,
    ReservationStorageError,
)
from greenfloor.daemon.testing.strategy_state import (
    inject_reseed_action_if_no_active_offers,
    strategy_config_from_market,
    strategy_state,
)
from greenfloor.daemon.testing.watchlist import (
    active_offer_counts_by_size,
    active_offer_counts_by_size_and_side,
    build_dexie_size_by_offer_id,
    match_watched_coin_ids,
    set_watched_coin_ids_for_market,
    update_market_coin_watchlist_from_dexie,
)

__all__ = [
    "AssetReservationCoordinator",
    "CoinsetAdapter",
    "MarketCycleResult",
    "MarketDispatchState",
    "PlannedAction",
    "POST_COOLDOWN_UNTIL",
    "CANCEL_COOLDOWN_UNTIL",
    "ReservationContentionError",
    "ReservationStorageError",
    "active_offer_counts_by_size",
    "active_offer_counts_by_size_and_side",
    "build_dexie_size_by_offer_id",
    "cancel_retry_config",
    "coinset_spendable_base_unit_coin_amounts",
    "consume_reload_marker",
    "cooldown_remaining_ms",
    "cooldowns",
    "drop_zero_repeat_strategy_actions",
    "execute_cancel_policy",
    "expand_planned_actions",
    "inject_reseed_action_if_no_active_offers",
    "inventory_scan",
    "main",
    "match_watched_coin_ids",
    "post_retry_config",
    "reconcile_market_cycle_offers",
    "resolve_quote_asset_for_offer",
    "run_loop",
    "run_once",
    "set_cooldown",
    "set_watched_coin_ids_for_market",
    "single_input_preferred_skip_reason",
    "strategy_config_from_market",
    "strategy_state",
    "update_market_coin_watchlist_from_dexie",
]
