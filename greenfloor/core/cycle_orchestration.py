"""Rust-backed daemon market cycle orchestration policy."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters import cycle_kernel

MARKET_CYCLE_PHASES: tuple[str, ...] = (
    "reconcile",
    "inventory",
    "strategy",
    "cancel",
    "coin_ops",
)


def select_market_batch(
    *,
    enabled_market_ids: list[str],
    slot_count: int,
    cursor: int,
    immediate_requeue_ids: list[str],
) -> dict[str, Any]:
    return cycle_kernel.select_market_batch(
        enabled_market_ids=enabled_market_ids,
        slot_count=slot_count,
        cursor=cursor,
        immediate_requeue_ids=immediate_requeue_ids,
    )


def enqueue_immediate_requeue(
    immediate_requeue_ids: list[str],
    market_id: str,
) -> list[str]:
    return cycle_kernel.enqueue_immediate_requeue(immediate_requeue_ids, market_id)


def should_use_market_slot_dispatch(*, enabled_market_count: int, slot_count: int) -> bool:
    return cycle_kernel.should_use_market_slot_dispatch(
        enabled_market_count=enabled_market_count,
        slot_count=slot_count,
    )


def dedupe_sorted_market_ids(market_ids: list[str]) -> list[str]:
    return cycle_kernel.dedupe_sorted_market_ids(market_ids)


def should_log_disabled_market(*, now_monotonic: float, next_log_deadline: float) -> bool:
    return cycle_kernel.should_log_disabled_market(
        now_monotonic=now_monotonic,
        next_log_deadline=next_log_deadline,
    )


def next_disabled_market_log_deadline(*, now_monotonic: float, interval_seconds: int) -> float:
    return cycle_kernel.next_disabled_market_log_deadline(
        now_monotonic=now_monotonic,
        interval_seconds=interval_seconds,
    )


def should_try_cat_inventory_fallback(*, coinset_scan_empty: bool, base_asset: str) -> bool:
    return cycle_kernel.should_try_cat_inventory_fallback(
        coinset_scan_empty=coinset_scan_empty,
        base_asset=base_asset,
    )


def collect_stale_sweep_candidates(
    *,
    rows: list[dict[str, Any]],
    enabled_market_ids: list[str],
    per_market_limit: int,
) -> list[dict[str, Any]]:
    return cycle_kernel.collect_stale_sweep_candidates(
        rows=rows,
        enabled_market_ids=enabled_market_ids,
        per_market_limit=per_market_limit,
    )


def classify_dexie_stale_offer_status(status: int) -> str | None:
    return cycle_kernel.classify_dexie_stale_offer_status(status)


def is_dexie_offer_missing_error_text(error_text: str) -> bool:
    return cycle_kernel.is_dexie_offer_missing_error_text(error_text)


def record_stale_sweep_check(
    *,
    progress: dict[str, Any],
    hit: dict[str, str] | None,
) -> dict[str, Any]:
    return cycle_kernel.record_stale_sweep_check(
        progress=progress,
        hit=hit,
    )


def empty_stale_sweep_payload() -> dict[str, Any]:
    return {
        "checked_offer_count": 0,
        "requeue_market_ids": [],
        "hits": [],
        "truncated": False,
    }
