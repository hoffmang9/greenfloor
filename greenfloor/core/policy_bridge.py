"""Rust-backed offer-build and retry policy (single canonical bridge)."""

from __future__ import annotations

from typing import Any

from greenfloor.core import kernel_bridge


def _python_bootstrap_block_error(
    *,
    bootstrap_status: str,
    bootstrap_reason: str,
    bootstrap_ready: bool,
) -> str | None:
    status = str(bootstrap_status).strip().lower()
    reason = str(bootstrap_reason).strip() or "bootstrap_precheck_failed"
    if status == "failed":
        return f"bootstrap_failed:{reason}"
    if status == "executed" and not bool(bootstrap_ready):
        return f"bootstrap_pending:{reason}"
    if status == "skipped" and reason != "already_ready":
        return f"bootstrap_precheck_skipped:{reason}"
    return None


def _dexie_row_matches_expected(
    row: object,
    *,
    expected_asset: str,
    expected_symbol: str,
) -> bool:
    if not isinstance(row, dict):
        return False
    row_id = str(row.get("id", "")).strip().lower()
    if row_id == expected_asset:
        return True
    if not expected_symbol:
        return False
    return (
        str(row.get("code", "")).strip().lower() == expected_symbol
        or str(row.get("name", "")).strip().lower() == expected_symbol
    )


def _python_dexie_offer_asset_expectation_error(
    *,
    offered: object,
    requested: object,
    expected_offered_asset_id: str,
    expected_offered_symbol: str,
    expected_requested_asset_id: str,
    expected_requested_symbol: str,
) -> str | None:
    expected_offered_asset = str(expected_offered_asset_id).strip().lower()
    expected_offered = str(expected_offered_symbol).strip().lower()
    if expected_offered_asset and isinstance(offered, list):
        if not any(
            _dexie_row_matches_expected(
                row,
                expected_asset=expected_offered_asset,
                expected_symbol=expected_offered,
            )
            for row in offered
        ):
            return (
                "dexie_offer_offered_asset_missing:"
                f"expected_asset={expected_offered_asset_id}:"
                f"expected_symbol={expected_offered_symbol}"
            )

    expected_requested_asset = str(expected_requested_asset_id).strip().lower()
    expected_requested = str(expected_requested_symbol).strip().lower()
    if expected_requested_asset and isinstance(requested, list):
        if not any(
            _dexie_row_matches_expected(
                row,
                expected_asset=expected_requested_asset,
                expected_symbol=expected_requested,
            )
            for row in requested
        ):
            return (
                "dexie_offer_requested_asset_missing:"
                f"expected_asset={expected_requested_asset_id}:"
                f"expected_symbol={expected_requested_symbol}"
            )
    return None


def _python_expected_publish_asset_fields(
    *,
    side: str,
    base_symbol: str,
    quote_asset: str,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
) -> dict[str, str]:
    is_buy = str(side).strip().lower() == "buy"
    if is_buy:
        return {
            "expected_offered_asset_id": str(resolved_quote_asset_id),
            "expected_offered_symbol": str(quote_asset),
            "expected_requested_asset_id": str(resolved_base_asset_id),
            "expected_requested_symbol": str(base_symbol),
        }
    return {
        "expected_offered_asset_id": str(resolved_base_asset_id),
        "expected_offered_symbol": str(base_symbol),
        "expected_requested_asset_id": str(resolved_quote_asset_id),
        "expected_requested_symbol": str(quote_asset),
    }


def resolve_offer_expiry_for_pricing(pricing: dict[str, Any]) -> tuple[str, int]:
    unit, value = kernel_bridge.policy_kernel().resolve_offer_expiry_for_pricing(pricing)
    return str(unit), int(value)


def resolve_quote_price_for_pricing(pricing: dict[str, Any]) -> float:
    return float(kernel_bridge.policy_kernel().resolve_quote_price_for_pricing(pricing))


def mojo_multiplier_for_leg(pricing: dict[str, Any], field: str, asset_id: str) -> int:
    return int(kernel_bridge.policy_kernel().mojo_multiplier_for_leg(pricing, field, asset_id))


def verify_offer_for_dexie(offer_text: str) -> str | None:
    try:
        error = kernel_bridge.policy_kernel().verify_offer_for_dexie(offer_text)
    except ImportError:
        return "wallet_sdk_import_error:greenfloor_signer_unavailable"
    return None if error is None else str(error)


def bootstrap_block_error(
    *,
    bootstrap_status: str,
    bootstrap_reason: str,
    bootstrap_ready: bool,
) -> str | None:
    kernel = kernel_bridge.policy_kernel()
    if hasattr(kernel, "bootstrap_block_error"):
        error = kernel.bootstrap_block_error(
            str(bootstrap_status),
            str(bootstrap_reason),
            bool(bootstrap_ready),
        )
    else:
        error = _python_bootstrap_block_error(
            bootstrap_status=bootstrap_status,
            bootstrap_reason=bootstrap_reason,
            bootstrap_ready=bootstrap_ready,
        )
    return None if error is None else str(error)


def expected_publish_asset_fields(
    *,
    side: str,
    base_symbol: str,
    quote_asset: str,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
) -> dict[str, str]:
    kernel = kernel_bridge.policy_kernel()
    if hasattr(kernel, "expected_publish_asset_fields"):
        payload = kernel.expected_publish_asset_fields(
            str(side),
            str(base_symbol),
            str(quote_asset),
            str(resolved_base_asset_id),
            str(resolved_quote_asset_id),
        )
        if isinstance(payload, dict):
            return {
                "expected_offered_asset_id": str(payload.get("expected_offered_asset_id", "")),
                "expected_offered_symbol": str(payload.get("expected_offered_symbol", "")),
                "expected_requested_asset_id": str(payload.get("expected_requested_asset_id", "")),
                "expected_requested_symbol": str(payload.get("expected_requested_symbol", "")),
            }
    return _python_expected_publish_asset_fields(
        side=side,
        base_symbol=base_symbol,
        quote_asset=quote_asset,
        resolved_base_asset_id=resolved_base_asset_id,
        resolved_quote_asset_id=resolved_quote_asset_id,
    )


def dexie_offer_asset_expectation_error(
    *,
    offered: object,
    requested: object,
    expected_offered_asset_id: str,
    expected_offered_symbol: str,
    expected_requested_asset_id: str,
    expected_requested_symbol: str,
) -> str | None:
    kernel = kernel_bridge.policy_kernel()
    if hasattr(kernel, "dexie_offer_asset_expectation_error"):
        error = kernel.dexie_offer_asset_expectation_error(
            offered,
            requested,
            str(expected_offered_asset_id),
            str(expected_offered_symbol),
            str(expected_requested_asset_id),
            str(expected_requested_symbol),
        )
    else:
        error = _python_dexie_offer_asset_expectation_error(
            offered=offered,
            requested=requested,
            expected_offered_asset_id=expected_offered_asset_id,
            expected_offered_symbol=expected_offered_symbol,
            expected_requested_asset_id=expected_requested_asset_id,
            expected_requested_symbol=expected_requested_symbol,
        )
    return None if error is None else str(error)


def parse_rate_limit_retry_seconds(error_text: str) -> float | None:
    value = kernel_bridge.policy_kernel().parse_rate_limit_retry_seconds(error_text)
    return None if value is None else float(value)


def moderate_retry_sleep_seconds(
    *,
    current_sleep: float,
    rate_limit_wait: float | None,
) -> float:
    return float(
        kernel_bridge.policy_kernel().moderate_retry_sleep_seconds(
            float(current_sleep), rate_limit_wait
        )
    )


def moderate_retry_next_sleep(current_sleep: float) -> float:
    return float(kernel_bridge.policy_kernel().moderate_retry_next_sleep(float(current_sleep)))


def dexie_invalid_offer_should_retry(*, error: str, attempt: int, max_attempts: int) -> bool:
    return bool(
        kernel_bridge.policy_kernel().dexie_invalid_offer_should_retry(
            str(error),
            int(attempt),
            int(max_attempts),
        )
    )


def dexie_invalid_offer_retry_sleep(*, attempt: int, initial_sleep: float) -> float:
    return float(
        kernel_bridge.policy_kernel().dexie_invalid_offer_retry_sleep(
            int(attempt), float(initial_sleep)
        )
    )


def coinset_fee_lookup_retry_sleep(attempt: int) -> float:
    return float(kernel_bridge.policy_kernel().coinset_fee_lookup_retry_sleep(int(attempt)))


def poll_exponential_sleep_now(
    *,
    elapsed_seconds: int,
    timeout_seconds: int,
    sleep_seconds: float,
    initial_sleep: float,
    max_sleep: float,
) -> float | None:
    value = kernel_bridge.policy_kernel().poll_exponential_sleep_now(
        int(elapsed_seconds),
        int(timeout_seconds),
        float(sleep_seconds),
        float(initial_sleep),
        float(max_sleep),
    )
    return None if value is None else float(value)


def poll_exponential_advance_sleep(
    *,
    sleep_seconds: float,
    initial_sleep: float,
    max_sleep: float,
    multiplier: float,
) -> float:
    return float(
        kernel_bridge.policy_kernel().poll_exponential_advance_sleep(
            float(sleep_seconds),
            float(initial_sleep),
            float(max_sleep),
            float(multiplier),
        )
    )
