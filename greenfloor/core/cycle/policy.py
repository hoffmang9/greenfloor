"""Python policy surface: payload helpers and exception-shaped wrappers."""

from __future__ import annotations

from typing import Any

from greenfloor.core.cycle_orchestration import StaleSweepProgress
from greenfloor.core.cycle_reseed import ReseedGapPlan
from greenfloor.core.planned_action import PlannedAction
from greenfloor.core.strategy_types import MarketState, StrategyConfig

from . import _bridge_managed as _managed
from . import _bridge_orchestration as _orchestration

__all__ = [
    "aggregate_two_sided_offer_counts",
    "apply_offer_signal_payload",
    "classify_managed_transient_error",
    "empty_stale_sweep_payload",
    "evaluate_market_payload",
    "expiry_seconds_for_action",
    "is_managed_upstream_transient_error",
    "is_managed_worker_transient_error",
    "is_parallel_dispatch_transient_error",
    "one_sided_offer_counts_by_side",
    "plan_reseed_actions_from_gap",
    "resolve_inventory_scan_source",
    "resolve_tracked_sizes",
    "size_counts_to_signer",
]


def size_counts_to_signer(counts: dict[int, int]) -> dict[str, int]:
    """Serialize size-indexed counts for JSON-backed Rust strategy payloads."""
    return {str(size): int(count) for size, count in counts.items()}


def evaluate_market_payload(
    *,
    state: dict[str, Any],
    config: dict[str, Any],
) -> list[PlannedAction]:
    market_state = MarketState(
        ones=int(state["ones"]),
        tens=int(state["tens"]),
        hundreds=int(state["hundreds"]),
        xch_price_usd=state.get("xch_price_usd"),
        bucket_counts_by_size=(
            {int(k): int(v) for k, v in state["bucket_counts_by_size"].items()}
            if state.get("bucket_counts_by_size") is not None
            else None
        ),
    )
    strategy_config = StrategyConfig(
        pair=str(config["pair"]),
        ones_target=int(config.get("ones_target", 5)),
        tens_target=int(config.get("tens_target", 2)),
        hundreds_target=int(config.get("hundreds_target", 1)),
        target_spread_bps=config.get("target_spread_bps"),
        min_xch_price_usd=config.get("min_xch_price_usd"),
        max_xch_price_usd=config.get("max_xch_price_usd"),
        offer_expiry_minutes=config.get("offer_expiry_minutes"),
        target_counts_by_size=(
            {int(k): int(v) for k, v in config["target_counts_by_size"].items()}
            if config.get("target_counts_by_size") is not None
            else None
        ),
    )
    return _orchestration.evaluate_market(state=market_state, config=strategy_config)


def apply_offer_signal_payload(*, state: str, signal: str) -> dict[str, Any]:
    return _orchestration.apply_offer_signal(state=state, signal=signal)


def expiry_seconds_for_action(action: Any) -> int | None:
    unit = str(getattr(action, "expiry_unit", "") or "").strip()
    try:
        value = int(getattr(action, "expiry_value", 0))
    except (TypeError, ValueError):
        return None
    return _orchestration.expiry_seconds_for_action(expiry_unit=unit, expiry_value=value)


def classify_managed_transient_error(exc: BaseException) -> str | None:
    return _managed.classify_managed_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_managed_upstream_transient_error(exc: BaseException) -> bool:
    return _managed.is_managed_upstream_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_managed_worker_transient_error(exc: BaseException) -> bool:
    return _managed.is_managed_worker_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_parallel_dispatch_transient_error(exc: BaseException) -> bool:
    return _managed.is_parallel_dispatch_transient_error(
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
    return _orchestration.resolve_inventory_scan_source(
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
        for size in _orchestration.resolve_tracked_sizes(
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
    return _orchestration.aggregate_two_sided_offer_counts(
        buy_counts,
        sell_counts,
        [int(size) for size in tracked_sizes],
    )


def one_sided_offer_counts_by_side(
    *,
    sell_counts: dict[int, int],
    tracked_sizes: list[int],
) -> tuple[dict[int, int], dict[int, int]]:
    payload = _orchestration.one_sided_offer_counts_by_side(
        sell_counts,
        [int(size) for size in tracked_sizes],
    )
    return dict(payload["buy"]), dict(payload["sell"])


def empty_stale_sweep_payload() -> StaleSweepProgress:
    return StaleSweepProgress()


def plan_reseed_actions_from_gap(
    *,
    strategy_actions: list[PlannedAction],
    active_counts_by_size: dict[int, int],
    target_counts_by_size: dict[int, int],
    strategy_config: StrategyConfig,
    xch_price_usd: float | None,
) -> ReseedGapPlan:
    result = _orchestration.plan_reseed_actions_from_gap(
        strategy_actions=strategy_actions,
        active_counts_by_size=active_counts_by_size,
        target_counts_by_size=target_counts_by_size,
        strategy_config=strategy_config,
        xch_price_usd=xch_price_usd,
    )
    if not isinstance(result, ReseedGapPlan):
        raise TypeError("plan_reseed_actions_from_gap returned non-ReseedGapPlan result")
    return result
