"""Strategy reseed injection."""

from __future__ import annotations

from datetime import datetime

from greenfloor.core.strategy import PlannedAction, StrategyConfig
from greenfloor.daemon import strategy_state
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.daemon.watchlist import (
    _ACTIVE_OFFER_STATES_FOR_RESEED,
    _RESEED_MEMPOOL_MAX_AGE_SECONDS,
    _active_offer_counts_by_size,
    _strategy_target_counts_by_size,
)
from greenfloor.storage.sqlite import SqliteStore


def _inject_reseed_action_if_no_active_offers(
    *,
    strategy_actions: list[PlannedAction],
    strategy_config: StrategyConfig,
    market,
    store: SqliteStore,
    xch_price_usd: float | None,
    clock: datetime,
    dexie_size_by_offer_id: dict[str, int] | None = None,
) -> list[PlannedAction]:
    if strategy_actions:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="strategy_actions_present",
            action_count=len(strategy_actions),
        )
        return strategy_actions
    target_by_size = _strategy_target_counts_by_size(strategy_config)
    active_counts_by_size, state_counts, active_unmapped_offer_ids = _active_offer_counts_by_size(
        store=store,
        market_id=market.market_id,
        clock=clock,
        dexie_size_by_offer_id=dexie_size_by_offer_id,
        tracked_sizes=set(target_by_size.keys()),
    )
    missing_by_size = {
        size: max(0, int(target_by_size.get(size, 0)) - int(active_counts_by_size.get(size, 0)))
        for size in target_by_size
    }
    if sum(missing_by_size.values()) <= 0:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="active_offer_targets_satisfied",
            active_states=sorted(_ACTIVE_OFFER_STATES_FOR_RESEED),
            recent_mempool_window_seconds=_RESEED_MEMPOOL_MAX_AGE_SECONDS,
            state_counts=state_counts,
            active_counts_by_size=active_counts_by_size,
            target_counts_by_size=target_by_size,
            active_unmapped_offer_ids=active_unmapped_offer_ids,
        )
        return strategy_actions

    seed_candidates = strategy_state.evaluate_reseed_candidates(
        strategy_config=strategy_config,
        xch_price_usd=xch_price_usd,
    )
    if not seed_candidates:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="no_seed_candidates",
            pair=strategy_config.pair,
            xch_price_usd=xch_price_usd,
        )
        return strategy_actions

    # Reseed one action per ladder size so the market rehydrates as 1/10/100,
    # not only the smallest denomination.
    one_per_size: dict[int, PlannedAction] = {}
    for candidate in seed_candidates:
        size = int(candidate.size)
        if size not in one_per_size:
            one_per_size[size] = candidate
    reseed_actions: list[PlannedAction] = []
    for size in sorted(one_per_size):
        missing = int(missing_by_size.get(size, 0))
        if missing <= 0:
            continue
        action = one_per_size[size]
        reseed_actions.append(
            PlannedAction(
                size=int(action.size),
                repeat=int(missing),
                pair=action.pair,
                expiry_unit=action.expiry_unit,
                expiry_value=int(action.expiry_value),
                cancel_after_create=action.cancel_after_create,
                reason="offer_size_gap_reseed",
                target_spread_bps=action.target_spread_bps,
            )
        )
    if not reseed_actions:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="missing_sizes_no_seed_template",
            missing_by_size=missing_by_size,
            candidate_sizes=sorted(one_per_size),
        )
        return strategy_actions
    reseed_actions = [action for action in reseed_actions if int(action.repeat) > 0]
    if not reseed_actions:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="reseed_zero_repeat_filtered",
            missing_by_size=missing_by_size,
        )
        return strategy_actions

    _log_market_decision(
        market.market_id,
        "reseed_injected",
        reason="offer_size_gap_reseed",
        sizes=[int(action.size) for action in reseed_actions],
        repeats=[int(action.repeat) for action in reseed_actions],
        action_count=sum(int(action.repeat) for action in reseed_actions),
        active_counts_by_size=active_counts_by_size,
        target_counts_by_size=target_by_size,
        missing_by_size=missing_by_size,
        pair=strategy_config.pair,
        expiry_unit=reseed_actions[0].expiry_unit,
        expiry_value=int(reseed_actions[0].expiry_value),
        cadence_limited_sizes=[],
    )
    return reseed_actions
