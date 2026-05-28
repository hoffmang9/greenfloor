"""Rust-backed daemon cycle policy (single Python surface)."""

from __future__ import annotations

import importlib
from typing import Any

_INSTALL_HINT = (
    "Install the greenfloor_signer extension (for example: "
    "`maturin develop -m greenfloor-signer-pyo3` from the repo root)."
)


def _import_signer() -> Any:
    try:
        return importlib.import_module("greenfloor_signer")
    except ImportError as exc:
        raise ImportError(
            f"greenfloor_signer is not available. {_INSTALL_HINT} Original error: {exc}"
        ) from exc


def _size_counts_to_signer(counts: dict[int, int]) -> dict[str, int]:
    return {str(size): int(count) for size, count in counts.items()}


def _size_counts_from_signer(counts: dict[str, int]) -> dict[int, int]:
    return {int(size): int(count) for size, count in counts.items()}


def _normalize_spendable_profiles(
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> dict[str, dict[str, int | bool]]:
    return {
        str(asset_id): {
            "total": int(profile.get("total", 0)),
            "max_single": int(profile.get("max_single", 0)),
            "max_single_known": bool(int(profile.get("max_single_known", 0))),
        }
        for asset_id, profile in spendable_profiles.items()
    }


__all__ = [
    "MARKET_CYCLE_PHASES",
    "aggregate_two_sided_offer_counts",
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
]


def market_cycle_phases() -> tuple[str, ...]:
    return tuple(_import_signer().market_cycle_phases())


MARKET_CYCLE_PHASES: tuple[str, ...] = market_cycle_phases()


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
    return _signer_expiry_seconds_for_action(expiry_unit=unit, expiry_value=value)


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
    return _signer_reservation_request_for_managed_offer(
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


def single_input_preferred_skip_reason(
    *,
    requested_amounts: dict[str, int],
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> str | None:
    return _signer_single_input_preferred_skip_reason(
        requested_amounts=requested_amounts,
        spendable_profiles=spendable_profiles,
    )


def is_transient_managed_upstream_error_text(error_text: str) -> bool:
    return _signer_is_transient_managed_upstream_error_text(error_text)


def classify_managed_transient_error(exc: BaseException) -> str | None:
    return _signer_classify_managed_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_managed_upstream_transient_error(exc: BaseException) -> bool:
    return _signer_is_managed_upstream_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_managed_worker_transient_error(exc: BaseException) -> bool:
    return _signer_is_managed_worker_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_parallel_dispatch_transient_error(exc: BaseException) -> bool:
    return _signer_is_parallel_dispatch_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_transient_dexie_visibility_404_error(error: str) -> bool:
    return _signer_is_transient_dexie_visibility_404_error(error)


def can_parallelize_managed_offers(
    *,
    signer_path_configured: bool,
    parallelism_enabled: bool,
    runtime_dry_run: bool,
    has_coordinator: bool,
) -> bool:
    return _signer_can_parallelize_managed_offers(
        signer_path_configured=signer_path_configured,
        parallelism_enabled=parallelism_enabled,
        runtime_dry_run=runtime_dry_run,
        has_coordinator=has_coordinator,
    )


def parallel_max_workers(*, submission_count: int, configured_max: int) -> int:
    return _signer_parallel_max_workers(
        submission_count=submission_count,
        configured_max=configured_max,
    )


def reservation_release_status(*, is_executed: bool) -> str:
    return _signer_reservation_release_status(is_executed=is_executed)


def should_apply_parallel_transient_cooldown(
    *,
    transient_failures: int,
    total_parallel: int,
    cooldown_seconds: int,
) -> bool:
    return _signer_should_apply_parallel_transient_cooldown(
        transient_failures=transient_failures,
        total_parallel=total_parallel,
        cooldown_seconds=cooldown_seconds,
    )


def managed_retry_sleep_ms(*, attempt_index: int, backoff_ms: int) -> int:
    return _signer_managed_retry_sleep_ms(
        attempt_index=attempt_index,
        backoff_ms=backoff_ms,
    )


def should_retry_managed_post(
    *,
    attempt_index: int,
    attempts_max: int,
    is_upstream_transient: bool,
) -> bool:
    return _signer_should_retry_managed_post(
        attempt_index=attempt_index,
        attempts_max=attempts_max,
        is_upstream_transient=is_upstream_transient,
    )


def prepare_parallel_managed_submission_decision(
    *,
    requested_amounts: dict[str, int],
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> dict[str, Any]:
    return _signer_prepare_parallel_managed_submission_decision(
        requested_amounts=requested_amounts,
        spendable_profiles=spendable_profiles,
    )


def classify_managed_post_result(
    *,
    success: bool,
    error_text: str,
    offer_id: str,
    publish_venue: str,
) -> dict[str, Any]:
    return _signer_classify_managed_post_result(
        success=success,
        error_text=error_text,
        offer_id=offer_id,
        publish_venue=publish_venue,
    )


def classify_dexie_visibility_outcome(
    *,
    visible: bool,
    visibility_error: str,
) -> dict[str, Any]:
    return _signer_classify_dexie_visibility_outcome(
        visible=visible,
        visibility_error=visibility_error,
    )


def count_parallel_transient_failures(items: list[dict[str, Any]]) -> int:
    return _signer_count_parallel_transient_failures(items)


def select_market_batch(
    *,
    enabled_market_ids: list[str],
    slot_count: int,
    cursor: int,
    immediate_requeue_ids: list[str],
) -> dict[str, Any]:
    return _signer_select_market_batch(
        enabled_market_ids=enabled_market_ids,
        slot_count=slot_count,
        cursor=cursor,
        immediate_requeue_ids=immediate_requeue_ids,
    )


def enqueue_immediate_requeue(
    immediate_requeue_ids: list[str],
    market_id: str,
) -> list[str]:
    return _signer_enqueue_immediate_requeue(immediate_requeue_ids, market_id)


def should_use_market_slot_dispatch(*, enabled_market_count: int, slot_count: int) -> bool:
    return _signer_should_use_market_slot_dispatch(
        enabled_market_count=enabled_market_count,
        slot_count=slot_count,
    )


def dedupe_sorted_market_ids(market_ids: list[str]) -> list[str]:
    return _signer_dedupe_sorted_market_ids(market_ids)


def should_log_disabled_market(*, now_monotonic: float, next_log_deadline: float) -> bool:
    return _signer_should_log_disabled_market(
        now_monotonic=now_monotonic,
        next_log_deadline=next_log_deadline,
    )


def next_disabled_market_log_deadline(*, now_monotonic: float, interval_seconds: int) -> float:
    return _signer_next_disabled_market_log_deadline(
        now_monotonic=now_monotonic,
        interval_seconds=interval_seconds,
    )


def should_try_cat_inventory_fallback(*, coinset_scan_empty: bool, base_asset: str) -> bool:
    return _signer_should_try_cat_inventory_fallback(
        coinset_scan_empty=coinset_scan_empty,
        base_asset=base_asset,
    )


def collect_stale_sweep_candidates(
    *,
    rows: list[dict[str, Any]],
    enabled_market_ids: list[str],
    per_market_limit: int,
) -> list[dict[str, Any]]:
    return _signer_collect_stale_sweep_candidates(
        rows=rows,
        enabled_market_ids=enabled_market_ids,
        per_market_limit=per_market_limit,
    )


def classify_dexie_stale_offer_status(status: int) -> str | None:
    return _signer_classify_dexie_stale_offer_status(status)


def is_dexie_offer_missing_error_text(error_text: str) -> bool:
    return _signer_is_dexie_offer_missing_error_text(error_text)


def record_stale_sweep_check(
    *,
    progress: dict[str, Any],
    hit: dict[str, str] | None,
) -> dict[str, Any]:
    return _signer_record_stale_sweep_check(
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


def needs_inventory_fallback(*, bucket_counts_available: bool, coinset_scan_empty: bool) -> bool:
    return _signer_needs_inventory_fallback(
        bucket_counts_available=bucket_counts_available,
        coinset_scan_empty=coinset_scan_empty,
    )


def resolve_inventory_scan_source(
    *,
    coinset_scan_found_coins: bool,
    coinset_scan_empty: bool,
    cat_scan_found_coins: bool,
    wallet_scan_found_coins: bool,
) -> str:
    return _signer_resolve_inventory_scan_source(
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
        for size in _signer_resolve_tracked_sizes(
            [int(size) for size in ladder_sizes],
            [int(size) for size in strategy_default_sizes],
        )
    ]


def is_two_sided_market_mode(market_mode: str) -> bool:
    return bool(_signer_is_two_sided_market_mode(str(market_mode)))


def aggregate_two_sided_offer_counts(
    *,
    buy_counts: dict[int, int],
    sell_counts: dict[int, int],
    tracked_sizes: list[int],
) -> dict[int, int]:
    payload = _signer_aggregate_two_sided_offer_counts(
        _size_counts_to_signer(buy_counts),
        _size_counts_to_signer(sell_counts),
        [int(size) for size in tracked_sizes],
    )
    return _size_counts_from_signer(payload)


def one_sided_offer_counts_by_side(
    *,
    sell_counts: dict[int, int],
    tracked_sizes: list[int],
) -> tuple[dict[int, int], dict[int, int]]:
    payload = _signer_one_sided_offer_counts_by_side(
        _size_counts_to_signer(sell_counts),
        [int(size) for size in tracked_sizes],
    )
    buy = _size_counts_from_signer(dict(payload["buy"]))
    sell = _size_counts_from_signer(dict(payload["sell"]))
    return buy, sell

# --- PyO3 bridge (internal) ---

def _signer_evaluate_market(*, state: dict[str, Any], config: dict[str, Any]) -> list[dict[str, Any]]:
    signer = _import_signer()
    result = signer.evaluate_market(state, config)
    if not isinstance(result, list):
        raise TypeError("evaluate_market returned non-list result")
    return [dict(item) for item in result]


def _signer_apply_offer_signal(*, state: str, signal: str) -> dict[str, Any]:
    signer = _import_signer()
    result = signer.apply_offer_signal(state, signal)
    if not isinstance(result, dict):
        raise TypeError("apply_offer_signal returned non-dict result")
    return dict(result)


def _signer_expiry_seconds_for_action(*, expiry_unit: str, expiry_value: int) -> int | None:
    signer = _import_signer()
    return signer.expiry_seconds_for_action(expiry_unit, expiry_value)


def _signer_reservation_request_for_managed_offer(request: dict[str, Any]) -> dict[str, int]:
    signer = _import_signer()
    result = signer.reservation_request_for_managed_offer(request)
    if not isinstance(result, dict):
        raise TypeError("reservation_request_for_managed_offer returned non-dict result")
    return {str(key): int(value) for key, value in result.items()}


def _signer_single_input_preferred_skip_reason(
    *,
    requested_amounts: dict[str, int],
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> str | None:
    signer = _import_signer()
    return signer.single_input_preferred_skip_reason(
        requested_amounts,
        _normalize_spendable_profiles(spendable_profiles),
    )


def _signer_is_transient_managed_upstream_error_text(error_text: str) -> bool:
    return bool(_import_signer().is_transient_managed_upstream_error_text(error_text))


def _signer_classify_managed_transient_error(*, exception_class: str, error_text: str) -> str | None:
    return _import_signer().classify_managed_transient_error(exception_class, error_text)


def _signer_is_managed_upstream_transient_error(*, exception_class: str, error_text: str) -> bool:
    return bool(_import_signer().is_managed_upstream_transient_error(exception_class, error_text))


def _signer_is_managed_worker_transient_error(*, exception_class: str, error_text: str) -> bool:
    return bool(_import_signer().is_managed_worker_transient_error(exception_class, error_text))


def _signer_is_parallel_dispatch_transient_error(*, exception_class: str, error_text: str) -> bool:
    return bool(_import_signer().is_parallel_dispatch_transient_error(exception_class, error_text))


def _signer_is_transient_dexie_visibility_404_error(error: str) -> bool:
    return bool(_import_signer().is_transient_dexie_visibility_404_error(error))


def _signer_can_parallelize_managed_offers(
    *,
    signer_path_configured: bool,
    parallelism_enabled: bool,
    runtime_dry_run: bool,
    has_coordinator: bool,
) -> bool:
    return bool(
        _import_signer().can_parallelize_managed_offers(
            signer_path_configured,
            parallelism_enabled,
            runtime_dry_run,
            has_coordinator,
        )
    )


def _signer_parallel_max_workers(*, submission_count: int, configured_max: int) -> int:
    return int(_import_signer().parallel_max_workers(int(submission_count), int(configured_max)))


def _signer_reservation_release_status(*, is_executed: bool) -> str:
    return str(_import_signer().reservation_release_status(bool(is_executed)))


def _signer_should_apply_parallel_transient_cooldown(
    *,
    transient_failures: int,
    total_parallel: int,
    cooldown_seconds: int,
) -> bool:
    return bool(
        _import_signer().should_apply_parallel_transient_cooldown(
            int(transient_failures),
            int(total_parallel),
            int(cooldown_seconds),
        )
    )


def _signer_managed_retry_sleep_ms(*, attempt_index: int, backoff_ms: int) -> int:
    return int(_import_signer().managed_retry_sleep_ms(int(attempt_index), int(backoff_ms)))


def _signer_should_retry_managed_post(
    *,
    attempt_index: int,
    attempts_max: int,
    is_upstream_transient: bool,
) -> bool:
    return bool(
        _import_signer().should_retry_managed_post(
            int(attempt_index),
            int(attempts_max),
            bool(is_upstream_transient),
        )
    )


def _signer_prepare_parallel_managed_submission_decision(
    *,
    requested_amounts: dict[str, int],
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> dict[str, Any]:
    signer = _import_signer()
    result = signer.prepare_parallel_managed_submission_decision(
        requested_amounts,
        _normalize_spendable_profiles(spendable_profiles),
    )
    if not isinstance(result, dict):
        raise TypeError("prepare_parallel_managed_submission_decision returned non-dict result")
    return dict(result)


def _signer_classify_managed_post_result(
    *,
    success: bool,
    error_text: str,
    offer_id: str,
    publish_venue: str,
) -> dict[str, Any]:
    signer = _import_signer()
    result = signer.classify_managed_post_result(success, error_text, offer_id, publish_venue)
    if not isinstance(result, dict):
        raise TypeError("classify_managed_post_result returned non-dict result")
    return dict(result)


def _signer_classify_dexie_visibility_outcome(
    *,
    visible: bool,
    visibility_error: str,
) -> dict[str, Any]:
    signer = _import_signer()
    result = signer.classify_dexie_visibility_outcome(visible, visibility_error)
    if not isinstance(result, dict):
        raise TypeError("classify_dexie_visibility_outcome returned non-dict result")
    return dict(result)


def _signer_count_parallel_transient_failures(items: list[dict[str, Any]]) -> int:
    return int(_import_signer().count_parallel_transient_failures(items))


def _signer_select_market_batch(
    *,
    enabled_market_ids: list[str],
    slot_count: int,
    cursor: int,
    immediate_requeue_ids: list[str],
) -> dict[str, Any]:
    signer = _import_signer()
    result = signer.select_market_batch(
        enabled_market_ids,
        int(slot_count),
        int(cursor),
        immediate_requeue_ids,
    )
    if not isinstance(result, dict):
        raise TypeError("select_market_batch returned non-dict result")
    return dict(result)


def _signer_enqueue_immediate_requeue(
    immediate_requeue_ids: list[str],
    market_id: str,
) -> list[str]:
    return list(_import_signer().enqueue_immediate_requeue(immediate_requeue_ids, market_id))


def _signer_should_use_market_slot_dispatch(*, enabled_market_count: int, slot_count: int) -> bool:
    return bool(
        _import_signer().should_use_market_slot_dispatch(
            int(enabled_market_count),
            int(slot_count),
        )
    )


def _signer_dedupe_sorted_market_ids(market_ids: list[str]) -> list[str]:
    return list(_import_signer().dedupe_sorted_market_ids(market_ids))


def _signer_should_log_disabled_market(*, now_monotonic: float, next_log_deadline: float) -> bool:
    return bool(
        _import_signer().should_log_disabled_market(float(now_monotonic), float(next_log_deadline))
    )


def _signer_next_disabled_market_log_deadline(*, now_monotonic: float, interval_seconds: int) -> float:
    return float(
        _import_signer().next_disabled_market_log_deadline(
            float(now_monotonic),
            int(interval_seconds),
        )
    )


def _signer_should_try_cat_inventory_fallback(*, coinset_scan_empty: bool, base_asset: str) -> bool:
    return bool(
        _import_signer().should_try_cat_inventory_fallback(bool(coinset_scan_empty), base_asset)
    )


def _signer_collect_stale_sweep_candidates(
    *,
    rows: list[dict[str, Any]],
    enabled_market_ids: list[str],
    per_market_limit: int,
) -> list[dict[str, Any]]:
    signer = _import_signer()
    result = signer.collect_stale_sweep_candidates(rows, enabled_market_ids, int(per_market_limit))
    if not isinstance(result, list):
        raise TypeError("collect_stale_sweep_candidates returned non-list result")
    return [dict(item) for item in result]


def _signer_classify_dexie_stale_offer_status(status: int) -> str | None:
    return _import_signer().classify_dexie_stale_offer_status(int(status))


def _signer_is_dexie_offer_missing_error_text(error_text: str) -> bool:
    return bool(_import_signer().is_dexie_offer_missing_error_text(error_text))


def _signer_record_stale_sweep_check(
    *,
    progress: dict[str, Any],
    hit: dict[str, str] | None,
) -> dict[str, Any]:
    signer = _import_signer()
    result = signer.record_stale_sweep_check(progress, hit)
    if not isinstance(result, dict):
        raise TypeError("record_stale_sweep_check returned non-dict result")
    return dict(result)


def _signer_market_cycle_phases() -> list[str]:
    return list(_import_signer().market_cycle_phases())


def _signer_needs_inventory_fallback(*, bucket_counts_available: bool, coinset_scan_empty: bool) -> bool:
    return bool(
        _import_signer().needs_inventory_fallback(
            bool(bucket_counts_available),
            bool(coinset_scan_empty),
        )
    )


def _signer_resolve_inventory_scan_source(
    coinset_scan_found_coins: bool,
    coinset_scan_empty: bool,
    cat_scan_found_coins: bool,
    wallet_scan_found_coins: bool,
) -> str:
    return str(
        _import_signer().resolve_inventory_scan_source(
            bool(coinset_scan_found_coins),
            bool(coinset_scan_empty),
            bool(cat_scan_found_coins),
            bool(wallet_scan_found_coins),
        )
    )


def _signer_resolve_tracked_sizes(ladder_sizes: list[int], strategy_default_sizes: list[int]) -> list[int]:
    return [
        int(size)
        for size in _import_signer().resolve_tracked_sizes(
            [int(value) for value in ladder_sizes],
            [int(value) for value in strategy_default_sizes],
        )
    ]


def _signer_is_two_sided_market_mode(market_mode: str) -> bool:
    return bool(_import_signer().is_two_sided_market_mode(str(market_mode)))


def _signer_aggregate_two_sided_offer_counts(
    buy_counts: dict[str, int],
    sell_counts: dict[str, int],
    tracked_sizes: list[int],
) -> dict[str, int]:
    signer = _import_signer()
    result = signer.aggregate_two_sided_offer_counts(
        buy_counts,
        sell_counts,
        [int(size) for size in tracked_sizes],
    )
    if not isinstance(result, dict):
        raise TypeError("aggregate_two_sided_offer_counts returned non-dict result")
    return {str(key): int(value) for key, value in result.items()}


def _signer_one_sided_offer_counts_by_side(
    sell_counts: dict[str, int],
    tracked_sizes: list[int],
) -> dict[str, dict[str, int]]:
    signer = _import_signer()
    result = signer.one_sided_offer_counts_by_side(
        sell_counts, [int(size) for size in tracked_sizes]
    )
    if not isinstance(result, dict):
        raise TypeError("one_sided_offer_counts_by_side returned non-dict result")
    return {
        "buy": {str(key): int(value) for key, value in dict(result.get("buy", {})).items()},
        "sell": {str(key): int(value) for key, value in dict(result.get("sell", {})).items()},
    }
