"""Rust-backed transient retry and polling backoff policy."""

from __future__ import annotations

from greenfloor.core.kernel_bridge import policy_kernel


def parse_rate_limit_retry_seconds(error_text: str) -> float | None:
    value = policy_kernel().parse_rate_limit_retry_seconds(error_text)
    return None if value is None else float(value)


def moderate_retry_sleep_seconds(
    *,
    current_sleep: float,
    rate_limit_wait: float | None,
) -> float:
    return float(
        policy_kernel().moderate_retry_sleep_seconds(float(current_sleep), rate_limit_wait)
    )


def moderate_retry_next_sleep(current_sleep: float) -> float:
    return float(policy_kernel().moderate_retry_next_sleep(float(current_sleep)))


def dexie_invalid_offer_should_retry(*, error: str, attempt: int, max_attempts: int) -> bool:
    return bool(
        policy_kernel().dexie_invalid_offer_should_retry(
            str(error),
            int(attempt),
            int(max_attempts),
        )
    )


def dexie_invalid_offer_retry_sleep(*, attempt: int, initial_sleep: float) -> float:
    return float(
        policy_kernel().dexie_invalid_offer_retry_sleep(int(attempt), float(initial_sleep))
    )


def coinset_fee_lookup_retry_sleep(attempt: int) -> float:
    return float(policy_kernel().coinset_fee_lookup_retry_sleep(int(attempt)))


def poll_exponential_sleep_now(
    *,
    elapsed_seconds: int,
    timeout_seconds: int,
    sleep_seconds: float,
    initial_sleep: float,
    max_sleep: float,
) -> float | None:
    value = policy_kernel().poll_exponential_sleep_now(
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
        policy_kernel().poll_exponential_advance_sleep(
            float(sleep_seconds),
            float(initial_sleep),
            float(max_sleep),
            float(multiplier),
        )
    )
