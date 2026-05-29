"""Market-cycle orchestration PyO3 bridge wrappers."""

from __future__ import annotations

from typing import Any

from greenfloor.core.cycle_orchestration import (
    MarketBatchSelection,
    OfferStateRow,
    StaleSweepCandidate,
    StaleSweepHit,
    StaleSweepProgress,
)
from greenfloor.core.engine_bridge import policy_engine
from greenfloor.core.engine_maps import require_i64_i64_map, require_side_offer_count_maps
from greenfloor.core.planned_action import PlannedAction, planned_actions_from_signer_list

__all__ = [
    "aggregate_two_sided_offer_counts",
    "apply_offer_signal",
    "collect_stale_sweep_candidates",
    "classify_dexie_stale_offer_status",
    "dedupe_sorted_market_ids",
    "enqueue_immediate_requeue",
    "evaluate_market",
    "evaluate_two_sided_market_actions",
    "executed_sell_offer_counts_by_size",
    "expiry_seconds_for_action",
    "is_dexie_offer_missing_error_text",
    "is_two_sided_market_mode",
    "needs_inventory_fallback",
    "next_disabled_market_log_deadline",
    "one_sided_offer_counts_by_side",
    "plan_reseed_actions_from_gap",
    "record_stale_sweep_check",
    "reseed_skip_reason_labels",
    "resolve_inventory_scan_source",
    "resolve_tracked_sizes",
    "select_market_batch",
    "should_log_disabled_market",
    "should_try_cat_inventory_fallback",
    "should_use_market_slot_dispatch",
]


def evaluate_market(*, state: Any, config: Any) -> list[PlannedAction]:
    signer = policy_engine()
    return planned_actions_from_signer_list(signer.evaluate_market(state, config))


def evaluate_two_sided_market_actions(
    *,
    buy_state: Any,
    sell_state: Any,
    buy_config: Any,
    sell_config: Any,
) -> list[PlannedAction]:
    signer = policy_engine()
    return planned_actions_from_signer_list(
        signer.evaluate_two_sided_market_actions(
            buy_state,
            sell_state,
            buy_config,
            sell_config,
        )
    )


def reseed_skip_reason_labels() -> tuple[str, ...]:
    return tuple(str(label) for label in policy_engine().reseed_skip_reason_labels())


def plan_reseed_actions_from_gap(
    *,
    strategy_actions: list[PlannedAction],
    active_counts_by_size: dict[int, int],
    target_counts_by_size: dict[int, int],
    strategy_config: Any,
    xch_price_usd: float | None,
) -> Any:
    signer = policy_engine()
    return signer.plan_reseed_actions_from_gap(
        strategy_actions,
        active_counts_by_size,
        target_counts_by_size,
        strategy_config,
        xch_price_usd,
    )


def apply_offer_signal(*, state: str, signal: str) -> dict[str, Any]:
    signer = policy_engine()
    result = signer.apply_offer_signal(state, signal)
    if not isinstance(result, dict):
        raise TypeError("apply_offer_signal returned non-dict result")
    return dict(result)


def expiry_seconds_for_action(*, expiry_unit: str, expiry_value: int) -> int | None:
    signer = policy_engine()
    return signer.expiry_seconds_for_action(expiry_unit, expiry_value)


def select_market_batch(
    *,
    enabled_market_ids: list[str],
    slot_count: int,
    cursor: int,
    immediate_requeue_ids: list[str],
) -> MarketBatchSelection:
    signer = policy_engine()
    result = signer.select_market_batch(
        enabled_market_ids,
        int(slot_count),
        int(cursor),
        immediate_requeue_ids,
    )
    if not isinstance(result, MarketBatchSelection):
        raise TypeError("select_market_batch returned non-MarketBatchSelection result")
    return result


def enqueue_immediate_requeue(
    immediate_requeue_ids: list[str],
    market_id: str,
) -> list[str]:
    return list(policy_engine().enqueue_immediate_requeue(immediate_requeue_ids, market_id))


def should_use_market_slot_dispatch(*, enabled_market_count: int, slot_count: int) -> bool:
    return bool(
        policy_engine().should_use_market_slot_dispatch(
            int(enabled_market_count),
            int(slot_count),
        )
    )


def dedupe_sorted_market_ids(market_ids: list[str]) -> list[str]:
    return list(policy_engine().dedupe_sorted_market_ids(market_ids))


def should_log_disabled_market(*, now_monotonic: float, next_log_deadline: float) -> bool:
    return bool(
        policy_engine().should_log_disabled_market(float(now_monotonic), float(next_log_deadline))
    )


def next_disabled_market_log_deadline(*, now_monotonic: float, interval_seconds: int) -> float:
    return float(
        policy_engine().next_disabled_market_log_deadline(
            float(now_monotonic),
            int(interval_seconds),
        )
    )


def should_try_cat_inventory_fallback(*, coinset_scan_empty: bool, base_asset: str) -> bool:
    return bool(
        policy_engine().should_try_cat_inventory_fallback(bool(coinset_scan_empty), base_asset)
    )


def collect_stale_sweep_candidates(
    *,
    rows: list[OfferStateRow],
    enabled_market_ids: list[str],
    per_market_limit: int,
) -> list[StaleSweepCandidate]:
    signer = policy_engine()
    result = signer.collect_stale_sweep_candidates(rows, enabled_market_ids, int(per_market_limit))
    if not isinstance(result, list):
        raise TypeError("collect_stale_sweep_candidates returned non-list result")
    for index, item in enumerate(result):
        if not isinstance(item, StaleSweepCandidate):
            raise TypeError(
                f"stale sweep candidate list item {index} must be StaleSweepCandidate, "
                f"got {type(item).__name__}"
            )
    return result


def classify_dexie_stale_offer_status(status: int) -> str | None:
    return policy_engine().classify_dexie_stale_offer_status(int(status))


def is_dexie_offer_missing_error_text(error_text: str) -> bool:
    return bool(policy_engine().is_dexie_offer_missing_error_text(error_text))


def record_stale_sweep_check(
    *,
    progress: StaleSweepProgress,
    hit: StaleSweepHit | None,
) -> StaleSweepProgress:
    signer = policy_engine()
    result = signer.record_stale_sweep_check(progress, hit)
    if not isinstance(result, StaleSweepProgress):
        raise TypeError("record_stale_sweep_check returned non-StaleSweepProgress result")
    return result


def needs_inventory_fallback(*, bucket_counts_available: bool, coinset_scan_empty: bool) -> bool:
    return bool(
        policy_engine().needs_inventory_fallback(
            bool(bucket_counts_available),
            bool(coinset_scan_empty),
        )
    )


def resolve_inventory_scan_source(
    coinset_scan_found_coins: bool,
    coinset_scan_empty: bool,
    cat_scan_found_coins: bool,
    wallet_scan_found_coins: bool,
) -> str:
    return str(
        policy_engine().resolve_inventory_scan_source(
            bool(coinset_scan_found_coins),
            bool(coinset_scan_empty),
            bool(cat_scan_found_coins),
            bool(wallet_scan_found_coins),
        )
    )


def resolve_tracked_sizes(ladder_sizes: list[int], strategy_default_sizes: list[int]) -> list[int]:
    return [
        int(size)
        for size in policy_engine().resolve_tracked_sizes(
            [int(value) for value in ladder_sizes],
            [int(value) for value in strategy_default_sizes],
        )
    ]


def is_two_sided_market_mode(market_mode: str) -> bool:
    return bool(policy_engine().is_two_sided_market_mode(str(market_mode)))


def aggregate_two_sided_offer_counts(
    buy_counts: dict[int, int],
    sell_counts: dict[int, int],
    tracked_sizes: list[int],
) -> dict[int, int]:
    signer = policy_engine()
    result = signer.aggregate_two_sided_offer_counts(
        buy_counts,
        sell_counts,
        [int(size) for size in tracked_sizes],
    )
    return require_i64_i64_map(result, label="aggregate_two_sided_offer_counts")


def one_sided_offer_counts_by_side(
    sell_counts: dict[int, int],
    tracked_sizes: list[int],
) -> dict[str, dict[int, int]]:
    signer = policy_engine()
    result = signer.one_sided_offer_counts_by_side(
        sell_counts, [int(size) for size in tracked_sizes]
    )
    return require_side_offer_count_maps(
        result,
        label="one_sided_offer_counts_by_side",
    )


def executed_sell_offer_counts_by_size(action_items: list[Any]) -> dict[int, int]:
    result = policy_engine().executed_sell_offer_counts_by_size(action_items)
    return require_i64_i64_map(result, label="executed_sell_offer_counts_by_size")
