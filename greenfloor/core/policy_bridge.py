"""Rust-backed offer-build and retry policy (single canonical bridge)."""

from __future__ import annotations

from typing import Any

from greenfloor.core import kernel_bridge


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
