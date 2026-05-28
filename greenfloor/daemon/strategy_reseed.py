"""Strategy reseed injection."""

from __future__ import annotations

from datetime import datetime

from greenfloor.core.reseed import ReseedGapPlan, plan_reseed_actions_from_gap
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


def _log_reseed_plan(market_id: str, plan: ReseedGapPlan, *, extra: dict | None = None) -> None:
    payload = dict(extra or {})
    if plan.skip_reason is not None:
        payload["reason"] = plan.skip_reason
        _log_market_decision(market_id, "reseed_skip", **payload)
        return
    if not plan.actions:
        return
    first = plan.actions[0]
    _log_market_decision(
        market_id,
        "reseed_injected",
        reason="offer_size_gap_reseed",
        sizes=[int(action.size) for action in plan.actions],
        repeats=[int(action.repeat) for action in plan.actions],
        action_count=sum(int(action.repeat) for action in plan.actions),
        pair=first.pair,
        expiry_unit=first.expiry_unit,
        expiry_value=int(first.expiry_value),
        cadence_limited_sizes=[],
        **payload,
    )


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
    seed_candidates = strategy_state.evaluate_reseed_candidates(
        strategy_config=strategy_config,
        xch_price_usd=xch_price_usd,
    )
    plan = plan_reseed_actions_from_gap(
        strategy_actions=strategy_actions,
        active_counts_by_size=active_counts_by_size,
        target_counts_by_size=target_by_size,
        seed_candidates=seed_candidates,
    )
    if plan.skip_reason == "active_offer_targets_satisfied":
        _log_reseed_plan(
            market.market_id,
            plan,
            extra={
                "active_states": sorted(_ACTIVE_OFFER_STATES_FOR_RESEED),
                "recent_mempool_window_seconds": _RESEED_MEMPOOL_MAX_AGE_SECONDS,
                "state_counts": state_counts,
                "active_counts_by_size": active_counts_by_size,
                "target_counts_by_size": target_by_size,
                "active_unmapped_offer_ids": active_unmapped_offer_ids,
            },
        )
        return plan.actions
    if plan.skip_reason == "no_seed_candidates":
        _log_reseed_plan(
            market.market_id,
            plan,
            extra={"pair": strategy_config.pair, "xch_price_usd": xch_price_usd},
        )
        return plan.actions
    if plan.skip_reason == "missing_sizes_no_seed_template":
        _log_reseed_plan(
            market.market_id,
            plan,
            extra={
                "missing_by_size": missing_by_size,
                "candidate_sizes": sorted(
                    {int(candidate.size) for candidate in seed_candidates}
                ),
            },
        )
        return plan.actions
    if plan.skip_reason == "reseed_zero_repeat_filtered":
        _log_reseed_plan(
            market.market_id,
            plan,
            extra={"missing_by_size": missing_by_size},
        )
        return plan.actions
    if plan.skip_reason is None and plan.actions:
        _log_reseed_plan(
            market.market_id,
            plan,
            extra={
                "active_counts_by_size": active_counts_by_size,
                "target_counts_by_size": target_by_size,
                "missing_by_size": missing_by_size,
            },
        )
    return plan.actions
