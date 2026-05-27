"""Rust-backed per-market cycle phase policy."""

from __future__ import annotations

from dataclasses import asdict, is_dataclass
from typing import Any

from greenfloor.adapters import cycle_kernel
from greenfloor.core.cycle_orchestration import MARKET_CYCLE_PHASES, should_try_cat_inventory_fallback

__all__ = [
    "MARKET_CYCLE_PHASES",
    "aggregate_two_sided_offer_counts",
    "is_two_sided_market_mode",
    "merge_cancel_policy",
    "merge_strategy_execution",
    "needs_inventory_fallback",
    "one_sided_offer_counts_by_side",
    "record_phase_error",
    "resolve_inventory_scan_source",
    "resolve_tracked_sizes",
    "should_try_cat_inventory_fallback",
]


def market_cycle_phases() -> list[str]:
    return cycle_kernel.market_cycle_phases()


def resolve_inventory_scan_source(
    *,
    coinset_scan_found_coins: bool,
    coinset_scan_empty: bool,
    cat_scan_found_coins: bool,
    wallet_scan_found_coins: bool,
) -> str:
    return cycle_kernel.resolve_inventory_scan_source(
        bool(coinset_scan_found_coins),
        bool(coinset_scan_empty),
        bool(cat_scan_found_coins),
        bool(wallet_scan_found_coins),
    )


def needs_inventory_fallback(*, bucket_counts_available: bool, coinset_scan_empty: bool) -> bool:
    return cycle_kernel.needs_inventory_fallback(
        bucket_counts_available=bucket_counts_available,
        coinset_scan_empty=coinset_scan_empty,
    )


def resolve_tracked_sizes(
    *,
    ladder_sizes: list[int],
    strategy_default_sizes: list[int],
) -> list[int]:
    return [
        int(size)
        for size in cycle_kernel.resolve_tracked_sizes(
            [int(size) for size in ladder_sizes],
            [int(size) for size in strategy_default_sizes],
        )
    ]


def is_two_sided_market_mode(market_mode: str) -> bool:
    return bool(cycle_kernel.is_two_sided_market_mode(str(market_mode)))


def aggregate_two_sided_offer_counts(
    *,
    buy_counts: dict[int, int],
    sell_counts: dict[int, int],
    tracked_sizes: list[int],
) -> dict[int, int]:
    payload = cycle_kernel.aggregate_two_sided_offer_counts(
        {str(key): int(value) for key, value in buy_counts.items()},
        {str(key): int(value) for key, value in sell_counts.items()},
        [int(size) for size in tracked_sizes],
    )
    return {int(key): int(value) for key, value in payload.items()}


def one_sided_offer_counts_by_side(
    *,
    sell_counts: dict[int, int],
    tracked_sizes: list[int],
) -> tuple[dict[int, int], dict[int, int]]:
    payload = cycle_kernel.one_sided_offer_counts_by_side(
        {str(key): int(value) for key, value in sell_counts.items()},
        [int(size) for size in tracked_sizes],
    )
    buy = {int(key): int(value) for key, value in dict(payload["buy"]).items()}
    sell = {int(key): int(value) for key, value in dict(payload["sell"]).items()}
    return buy, sell


def _result_payload(result: Any) -> dict[str, Any]:
    if is_dataclass(result) and not isinstance(result, type):
        payload = asdict(result)
        payload["immediate_requeue_signals"] = list(payload.get("immediate_requeue_signals", []))
        return payload
    raise TypeError("result_must_be_dataclass")


def _apply_result_payload(result: Any, payload: dict[str, Any]) -> None:
    result.cycle_errors = int(payload.get("cycle_errors", 0))
    result.strategy_planned = int(payload.get("strategy_planned", 0))
    result.strategy_executed = int(payload.get("strategy_executed", 0))
    result.cancel_triggered = bool(payload.get("cancel_triggered", False))
    result.cancel_planned = int(payload.get("cancel_planned", 0))
    result.cancel_executed = int(payload.get("cancel_executed", 0))
    result.immediate_requeue_requested = bool(payload.get("immediate_requeue_requested", False))
    result.immediate_requeue_signals = list(payload.get("immediate_requeue_signals", []))


def merge_strategy_execution(result: Any, *, planned: int, executed: int) -> None:
    updated = cycle_kernel.merge_market_cycle_strategy_execution(
        _result_payload(result),
        int(planned),
        int(executed),
    )
    _apply_result_payload(result, updated)


def merge_cancel_policy(
    result: Any,
    *,
    triggered: bool,
    planned: int,
    executed: int,
) -> None:
    updated = cycle_kernel.merge_market_cycle_cancel_policy(
        _result_payload(result),
        bool(triggered),
        int(planned),
        int(executed),
    )
    _apply_result_payload(result, updated)


def record_phase_error(result: Any) -> None:
    updated = cycle_kernel.record_market_cycle_phase_error(_result_payload(result))
    _apply_result_payload(result, updated)
