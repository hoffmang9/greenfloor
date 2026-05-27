"""Stable public hooks for daemon unit and integration tests.

Production orchestration should import implementation modules directly. Tests should
prefer imports from this module so refactors do not require wide private-symbol churn.

Canonical monkeypatch targets (module-qualified paths):

- ``greenfloor.daemon.testing.main`` — cycle orchestration (`run_once`, adapters)
- ``greenfloor.daemon.testing.inventory_scan.CoinsetAdapter`` — coinset adapter factory
- ``greenfloor.daemon.testing.strategy_dispatch`` — managed/local offer dispatch
- ``greenfloor.daemon.testing.strategy_state.evaluate_reseed_candidates`` — reseed policy
- ``greenfloor.daemon.testing.cooldowns`` — post/cancel cooldown globals
- ``greenfloor.daemon.testing.cancel_policy.execute_cancel_policy`` — cancel policy entry
"""

from __future__ import annotations

import greenfloor.daemon.cooldowns as cooldowns
import greenfloor.daemon.inventory_scan as inventory_scan
import greenfloor.daemon.main as main
import greenfloor.daemon.strategy_dispatch as strategy_dispatch
import greenfloor.daemon.strategy_state as strategy_state
from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.daemon.cancel_policy import _execute_cancel_policy_for_market as execute_cancel_policy
from greenfloor.daemon.cooldowns import (
    PENDING_VISIBILITY_REASON,
    _CANCEL_COOLDOWN_UNTIL as CANCEL_COOLDOWN_UNTIL,
    _POST_COOLDOWN_UNTIL as POST_COOLDOWN_UNTIL,
    _cancel_retry_config as cancel_retry_config,
    _cooldown_remaining_ms as cooldown_remaining_ms,
    _post_retry_config as post_retry_config,
    _set_cooldown as set_cooldown,
)
from greenfloor.daemon.inventory_scan import (
    _coinset_spendable_base_unit_coin_amounts as coinset_spendable_base_unit_coin_amounts,
)
from greenfloor.daemon.main import (
    _MarketCycleResult as MarketCycleResult,
    _MarketDispatchState as MarketDispatchState,
    _consume_reload_marker as consume_reload_marker,
    _detect_stale_open_offers_for_requeue as detect_stale_open_offers_for_requeue,
    _enqueue_immediate_requeue_market as enqueue_immediate_requeue_market,
    _select_market_batch as select_market_batch,
    run_once,
)
from greenfloor.daemon.market_helpers import (
    _resolve_quote_asset_for_offer as resolve_quote_asset_for_offer,
)
from greenfloor.daemon.offer_reconcile_cycle import reconcile_market_cycle_offers
from greenfloor.daemon.reservations import (
    AssetReservationCoordinator,
    ReservationContentionError,
    ReservationStorageError,
)
from greenfloor.daemon.strategy_dispatch import (
    _build_offer_for_action as build_offer_for_action,
    _execute_single_local_action as execute_single_local_action,
    _execute_strategy_actions as execute_strategy_actions,
    _expand_strategy_actions as expand_strategy_actions,
    _single_input_preferred_skip_reason as single_input_preferred_skip_reason,
)
from greenfloor.daemon.strategy_reseed import (
    _inject_reseed_action_if_no_active_offers as inject_reseed_action_if_no_active_offers,
)
from greenfloor.daemon.strategy_state import (
    _strategy_config_from_market as strategy_config_from_market,
    evaluate_reseed_candidates,
)
from greenfloor.daemon.watchlist import (
    _active_offer_counts_by_size as active_offer_counts_by_size,
    _active_offer_counts_by_size_and_side as active_offer_counts_by_size_and_side,
    _build_dexie_size_by_offer_id as build_dexie_size_by_offer_id,
    _match_watched_coin_ids as match_watched_coin_ids,
    _set_watched_coin_ids_for_market as set_watched_coin_ids_for_market,
    _update_market_coin_watchlist_from_dexie as update_market_coin_watchlist_from_dexie,
)
from greenfloor.core.strategy import PlannedAction


def drop_zero_repeat_strategy_actions(actions: list[PlannedAction]) -> list[PlannedAction]:
    return [action for action in actions if int(action.repeat) > 0]


__all__ = [
    "AssetReservationCoordinator",
    "CoinsetAdapter",
    "MarketCycleResult",
    "MarketDispatchState",
    "PENDING_VISIBILITY_REASON",
    "PlannedAction",
    "POST_COOLDOWN_UNTIL",
    "CANCEL_COOLDOWN_UNTIL",
    "ReservationContentionError",
    "ReservationStorageError",
    "active_offer_counts_by_size",
    "active_offer_counts_by_size_and_side",
    "build_dexie_size_by_offer_id",
    "build_offer_for_action",
    "cancel_retry_config",
    "coinset_spendable_base_unit_coin_amounts",
    "consume_reload_marker",
    "cooldown_remaining_ms",
    "cooldowns",
    "detect_stale_open_offers_for_requeue",
    "drop_zero_repeat_strategy_actions",
    "enqueue_immediate_requeue_market",
    "evaluate_reseed_candidates",
    "execute_cancel_policy",
    "execute_single_local_action",
    "execute_strategy_actions",
    "expand_strategy_actions",
    "inject_reseed_action_if_no_active_offers",
    "inventory_scan",
    "main",
    "match_watched_coin_ids",
    "post_retry_config",
    "reconcile_market_cycle_offers",
    "resolve_quote_asset_for_offer",
    "run_once",
    "select_market_batch",
    "set_cooldown",
    "set_watched_coin_ids_for_market",
    "single_input_preferred_skip_reason",
    "strategy_config_from_market",
    "strategy_dispatch",
    "strategy_state",
    "update_market_coin_watchlist_from_dexie",
]
