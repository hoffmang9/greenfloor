"""Strategy reseed injection."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime

from greenfloor.core.cycle import plan_reseed_actions_from_gap
from greenfloor.core.cycle_reseed import ReseedGapPlan, ReseedSkipReason
from greenfloor.core.engine_bridge import import_engine
from greenfloor.core.strategy import PlannedAction, StrategyConfig
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.daemon.testing.watchlist import active_offer_counts_by_size
from greenfloor.storage.sqlite import SqliteStore

RESEED_MEMPOOL_MAX_AGE_SECONDS = int(import_engine().RESEED_MEMPOOL_MAX_AGE_SECONDS)

_ACTIVE_OFFER_STATES_FOR_RESEED = frozenset({"open", "refresh_due"})


def _strategy_target_counts_by_size(strategy_config: StrategyConfig) -> dict[int, int]:
    if strategy_config.target_counts_by_size:
        return {
            int(size): int(target)
            for size, target in sorted(strategy_config.target_counts_by_size.items())
            if int(size) > 0 and int(target) >= 0
        }
    return {
        1: int(strategy_config.ones_target),
        10: int(strategy_config.tens_target),
        100: int(strategy_config.hundreds_target),
    }


@dataclass(frozen=True, slots=True)
class _ReseedLogContext:
    strategy_config: StrategyConfig
    xch_price_usd: float | None
    state_counts: dict[str, int]
    active_counts_by_size: dict[int, int]
    target_counts_by_size: dict[int, int]
    active_unmapped_offer_ids: int


def _reseed_skip_log_extra(plan: ReseedGapPlan, ctx: _ReseedLogContext) -> dict:
    match plan.skip_reason:
        case ReseedSkipReason.STRATEGY_ACTIONS_PRESENT:
            return {"action_count": len(plan.actions)}
        case ReseedSkipReason.ACTIVE_OFFER_TARGETS_SATISFIED:
            return {
                "active_states": sorted(_ACTIVE_OFFER_STATES_FOR_RESEED),
                "recent_mempool_window_seconds": RESEED_MEMPOOL_MAX_AGE_SECONDS,
                "state_counts": ctx.state_counts,
                "active_counts_by_size": ctx.active_counts_by_size,
                "target_counts_by_size": ctx.target_counts_by_size,
                "active_unmapped_offer_ids": ctx.active_unmapped_offer_ids,
            }
        case ReseedSkipReason.NO_SEED_CANDIDATES:
            return {
                "pair": ctx.strategy_config.pair,
                "xch_price_usd": ctx.xch_price_usd,
            }
        case ReseedSkipReason.MISSING_SIZES_NO_SEED_TEMPLATE:
            return {
                "missing_by_size": plan.missing_by_size,
                "candidate_sizes": sorted(
                    size for size, missing in plan.missing_by_size.items() if missing > 0
                ),
            }
        case ReseedSkipReason.RESEED_ZERO_REPEAT_FILTERED:
            return {"missing_by_size": plan.missing_by_size}
        case None:
            if plan.actions:
                return {"missing_by_size": plan.missing_by_size}
    return {}


def _log_reseed_plan(market_id: str, plan: ReseedGapPlan, ctx: _ReseedLogContext) -> None:
    extra = _reseed_skip_log_extra(plan, ctx)
    if plan.skip_reason is not None:
        payload = dict(extra)
        payload["reason"] = plan.skip_reason.value
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
        **extra,
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
    target_by_size = _strategy_target_counts_by_size(strategy_config)
    active_counts_by_size, state_counts, active_unmapped_offer_ids = active_offer_counts_by_size(
        store=store,
        market_id=market.market_id,
        clock=clock,
        dexie_size_by_offer_id=dexie_size_by_offer_id,
        tracked_sizes=set(target_by_size.keys()),
    )
    plan = plan_reseed_actions_from_gap(
        strategy_actions=strategy_actions,
        active_counts_by_size=active_counts_by_size,
        target_counts_by_size=target_by_size,
        strategy_config=strategy_config,
        xch_price_usd=xch_price_usd,
    )
    _log_reseed_plan(
        market.market_id,
        plan,
        _ReseedLogContext(
            strategy_config=strategy_config,
            xch_price_usd=xch_price_usd,
            state_counts=state_counts,
            active_counts_by_size=active_counts_by_size,
            target_counts_by_size=target_by_size,
            active_unmapped_offer_ids=active_unmapped_offer_ids,
        ),
    )
    return plan.actions
