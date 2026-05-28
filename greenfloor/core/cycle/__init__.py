"""Rust-backed daemon cycle policy (single Python surface)."""

from __future__ import annotations

from typing import Any

from greenfloor.core.cycle import _bridge as bridge

__all__ = [
    "MARKET_CYCLE_PHASES",
    "aggregate_two_sided_offer_counts",
    "apply_offer_signal_payload",
    "can_parallelize_managed_offers",
    "classify_dexie_stale_offer_status",
    "classify_dexie_visibility_outcome",
    "classify_managed_post_result",
    "classify_managed_transient_error",
    "collect_stale_sweep_candidates",
    "count_parallel_transient_failures",
    "dedupe_sorted_market_ids",
    "empty_stale_sweep_payload",
    "enqueue_immediate_requeue",
    "evaluate_market_payload",
    "expand_strategy_actions",
    "expiry_seconds_for_action",
    "is_dexie_offer_missing_error_text",
    "is_managed_upstream_transient_error",
    "is_managed_worker_transient_error",
    "is_parallel_dispatch_transient_error",
    "is_transient_dexie_visibility_404_error",
    "is_transient_managed_upstream_error_text",
    "is_two_sided_market_mode",
    "managed_retry_sleep_ms",
    "market_cycle_phases",
    "needs_inventory_fallback",
    "next_disabled_market_log_deadline",
    "one_sided_offer_counts_by_side",
    "parallel_max_workers",
    "prepare_parallel_managed_submission_decision",
    "record_stale_sweep_check",
    "reservation_release_status",
    "reservation_request_for_managed_offer",
    "resolve_inventory_scan_source",
    "resolve_tracked_sizes",
    "select_market_batch",
    "should_apply_parallel_transient_cooldown",
    "should_log_disabled_market",
    "should_retry_managed_post",
    "should_try_cat_inventory_fallback",
    "should_use_market_slot_dispatch",
    "single_input_preferred_skip_reason",
    "size_counts_to_signer",
]

# Direct re-exports (public signature matches bridge).
can_parallelize_managed_offers = bridge.can_parallelize_managed_offers
classify_dexie_stale_offer_status = bridge.classify_dexie_stale_offer_status
classify_dexie_visibility_outcome = bridge.classify_dexie_visibility_outcome
classify_managed_post_result = bridge.classify_managed_post_result
collect_stale_sweep_candidates = bridge.collect_stale_sweep_candidates
count_parallel_transient_failures = bridge.count_parallel_transient_failures
dedupe_sorted_market_ids = bridge.dedupe_sorted_market_ids
enqueue_immediate_requeue = bridge.enqueue_immediate_requeue
is_dexie_offer_missing_error_text = bridge.is_dexie_offer_missing_error_text
is_transient_dexie_visibility_404_error = bridge.is_transient_dexie_visibility_404_error
is_transient_managed_upstream_error_text = bridge.is_transient_managed_upstream_error_text
is_two_sided_market_mode = bridge.is_two_sided_market_mode
managed_retry_sleep_ms = bridge.managed_retry_sleep_ms
needs_inventory_fallback = bridge.needs_inventory_fallback
next_disabled_market_log_deadline = bridge.next_disabled_market_log_deadline
parallel_max_workers = bridge.parallel_max_workers
prepare_parallel_managed_submission_decision = bridge.prepare_parallel_managed_submission_decision
record_stale_sweep_check = bridge.record_stale_sweep_check
reservation_release_status = bridge.reservation_release_status
select_market_batch = bridge.select_market_batch
should_apply_parallel_transient_cooldown = bridge.should_apply_parallel_transient_cooldown
should_log_disabled_market = bridge.should_log_disabled_market
should_retry_managed_post = bridge.should_retry_managed_post
should_try_cat_inventory_fallback = bridge.should_try_cat_inventory_fallback
should_use_market_slot_dispatch = bridge.should_use_market_slot_dispatch
single_input_preferred_skip_reason = bridge.single_input_preferred_skip_reason


def market_cycle_phases() -> tuple[str, ...]:
    return tuple(bridge._import_signer().market_cycle_phases())


MARKET_CYCLE_PHASES: tuple[str, ...] = market_cycle_phases()


def size_counts_to_signer(counts: dict[int, int]) -> dict[str, int]:
    """Serialize size-indexed counts for JSON-backed Rust strategy payloads."""
    return {str(size): int(count) for size, count in counts.items()}


def evaluate_market_payload(
    *,
    state: dict[str, Any],
    config: dict[str, Any],
) -> list[dict[str, Any]]:
    return bridge.evaluate_market(state=state, config=config)


def apply_offer_signal_payload(*, state: str, signal: str) -> dict[str, Any]:
    return bridge.apply_offer_signal(state=state, signal=signal)


def expand_strategy_actions(strategy_actions: list[Any]) -> list[Any]:
    expanded: list[Any] = []
    for action in strategy_actions:
        repeat = max(0, int(getattr(action, "repeat", 0)))
        expanded.extend(action for _ in range(repeat))
    return expanded


def expiry_seconds_for_action(action: Any) -> int | None:
    unit = str(getattr(action, "expiry_unit", "") or "").strip()
    try:
        value = int(getattr(action, "expiry_value", 0))
    except (TypeError, ValueError):
        return None
    return bridge.expiry_seconds_for_action(expiry_unit=unit, expiry_value=value)


def reservation_request_for_managed_offer(
    *,
    side: str,
    size_base_units: int,
    base_asset_id: str,
    quote_asset_id: str,
    base_unit_mojo_multiplier: int,
    quote_unit_mojo_multiplier: int,
    quote_price: float,
    fee_asset_id: str,
    fee_amount_mojos: int,
) -> dict[str, int]:
    return bridge.reservation_request_for_managed_offer(
        {
            "side": side,
            "size_base_units": int(size_base_units),
            "base_asset_id": str(base_asset_id),
            "quote_asset_id": str(quote_asset_id),
            "base_unit_mojo_multiplier": int(base_unit_mojo_multiplier),
            "quote_unit_mojo_multiplier": int(quote_unit_mojo_multiplier),
            "quote_price": float(quote_price),
            "fee_asset_id": str(fee_asset_id),
            "fee_amount_mojos": int(fee_amount_mojos),
        }
    )


def classify_managed_transient_error(exc: BaseException) -> str | None:
    return bridge.classify_managed_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_managed_upstream_transient_error(exc: BaseException) -> bool:
    return bridge.is_managed_upstream_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_managed_worker_transient_error(exc: BaseException) -> bool:
    return bridge.is_managed_worker_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_parallel_dispatch_transient_error(exc: BaseException) -> bool:
    return bridge.is_parallel_dispatch_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def resolve_inventory_scan_source(
    *,
    coinset_scan_found_coins: bool,
    coinset_scan_empty: bool,
    cat_scan_found_coins: bool,
    wallet_scan_found_coins: bool,
) -> str:
    return bridge.resolve_inventory_scan_source(
        bool(coinset_scan_found_coins),
        bool(coinset_scan_empty),
        bool(cat_scan_found_coins),
        bool(wallet_scan_found_coins),
    )


def resolve_tracked_sizes(
    *,
    ladder_sizes: list[int],
    strategy_default_sizes: list[int],
) -> list[int]:
    return [
        int(size)
        for size in bridge.resolve_tracked_sizes(
            [int(size) for size in ladder_sizes],
            [int(size) for size in strategy_default_sizes],
        )
    ]


def aggregate_two_sided_offer_counts(
    *,
    buy_counts: dict[int, int],
    sell_counts: dict[int, int],
    tracked_sizes: list[int],
) -> dict[int, int]:
    return bridge.aggregate_two_sided_offer_counts(
        buy_counts,
        sell_counts,
        [int(size) for size in tracked_sizes],
    )


def one_sided_offer_counts_by_side(
    *,
    sell_counts: dict[int, int],
    tracked_sizes: list[int],
) -> tuple[dict[int, int], dict[int, int]]:
    payload = bridge.one_sided_offer_counts_by_side(
        sell_counts,
        [int(size) for size in tracked_sizes],
    )
    return dict(payload["buy"]), dict(payload["sell"])


def empty_stale_sweep_payload() -> dict[str, Any]:
    return {
        "checked_offer_count": 0,
        "requeue_market_ids": [],
        "hits": [],
        "truncated": False,
    }
