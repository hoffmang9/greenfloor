"""Strategy evaluation: offer counts, planner, reseed injection."""

from __future__ import annotations

from datetime import datetime
from typing import Any

from greenfloor.core.cycle import (
    aggregate_two_sided_offer_counts,
    filter_planned_actions_with_positive_repeat,
    is_two_sided_market_mode,
    one_sided_offer_counts_by_side,
    resolve_tracked_sizes,
)
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.planned_action import PlannedAction
from greenfloor.core.strategy import evaluate_market
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.daemon.strategy_reseed import _inject_reseed_action_if_no_active_offers
from greenfloor.daemon.strategy_state import (
    _evaluate_two_sided_market_actions,
    _strategy_config_from_market,
    _strategy_state_from_bucket_counts,
)
from greenfloor.daemon.watchlist import (
    _active_offer_counts_by_size,
    _active_offer_counts_by_size_and_side,
    _strategy_target_counts_by_size,
)
from greenfloor.storage.sqlite import SqliteStore


def resolve_tracked_sizes_for_market(
    *, market: MarketConfig, strategy_config: Any
) -> list[int]:
    ladder_sizes = [
        int(getattr(entry, "size_base_units", 0))
        for side_entries in (getattr(market, "ladders", {}) or {}).values()
        for entry in side_entries
    ]
    return resolve_tracked_sizes(
        ladder_sizes=ladder_sizes,
        strategy_default_sizes=list(_strategy_target_counts_by_size(strategy_config).keys()),
    )


def evaluate_strategy_for_market(
    *,
    market: MarketConfig,
    store: SqliteStore,
    xch_price_usd: float | None,
    now: datetime,
    dexie_size_by_offer_id: dict[str, int],
) -> tuple[list[PlannedAction], dict[str, dict[int, int]], dict[int, int]]:
    market_mode = str(getattr(market, "mode", "")).strip().lower()
    strategy_config = _strategy_config_from_market(market)
    tracked_sizes_list = resolve_tracked_sizes_for_market(
        market=market,
        strategy_config=strategy_config,
    )
    tracked_sizes = set(tracked_sizes_list)
    two_sided = is_two_sided_market_mode(market_mode)
    if two_sided:
        offer_counts_by_side, offer_state_counts, active_unmapped_offer_ids = (
            _active_offer_counts_by_size_and_side(
                store=store,
                market_id=market.market_id,
                clock=now,
                dexie_size_by_offer_id=dexie_size_by_offer_id,
                tracked_sizes=tracked_sizes,
            )
        )
        active_offer_counts_by_size = aggregate_two_sided_offer_counts(
            buy_counts=offer_counts_by_side["buy"],
            sell_counts=offer_counts_by_side["sell"],
            tracked_sizes=tracked_sizes_list,
        )
    else:
        active_offer_counts_by_size, offer_state_counts, active_unmapped_offer_ids = (
            _active_offer_counts_by_size(
                store=store,
                market_id=market.market_id,
                clock=now,
                dexie_size_by_offer_id=dexie_size_by_offer_id,
                tracked_sizes=tracked_sizes,
            )
        )
        buy_side, sell_side = one_sided_offer_counts_by_side(
            sell_counts=active_offer_counts_by_size,
            tracked_sizes=tracked_sizes_list,
        )
        offer_counts_by_side = {"buy": buy_side, "sell": sell_side}
    _log_market_decision(
        market.market_id,
        "strategy_state_source",
        source="dexie_offer_coverage",
        active_offer_counts_by_size=active_offer_counts_by_size,
        active_offer_counts_by_side=offer_counts_by_side,
        state_counts=offer_state_counts,
        active_unmapped_offer_ids=active_unmapped_offer_ids,
    )
    if two_sided:
        strategy_actions = _evaluate_two_sided_market_actions(
            market=market,
            counts_by_side=offer_counts_by_side,
            xch_price_usd=xch_price_usd,
            now=now,
        )
    else:
        strategy_actions = evaluate_market(
            state=_strategy_state_from_bucket_counts(
                active_offer_counts_by_size, xch_price_usd=xch_price_usd
            ),
            config=strategy_config,
        )
    strategy_actions = filter_planned_actions_with_positive_repeat(strategy_actions)
    _log_market_decision(
        market.market_id,
        "strategy_evaluated",
        pair=strategy_config.pair,
        mode=market_mode or "sell_only",
        offer_counts=active_offer_counts_by_size,
        xch_price_usd=xch_price_usd,
        action_count=len(strategy_actions),
        cadence_limited_sizes=[],
    )
    if not two_sided:
        strategy_actions = _inject_reseed_action_if_no_active_offers(
            strategy_actions=strategy_actions,
            strategy_config=strategy_config,
            market=market,
            store=store,
            xch_price_usd=xch_price_usd,
            clock=now,
            dexie_size_by_offer_id=dexie_size_by_offer_id,
        )
    _log_market_decision(
        market.market_id,
        "strategy_after_reseed",
        action_count=len(strategy_actions),
        reseed_injected=any(
            str(action.reason) == "no_active_offer_reseed" for action in strategy_actions
        ),
    )
    return strategy_actions, offer_counts_by_side, active_offer_counts_by_size
