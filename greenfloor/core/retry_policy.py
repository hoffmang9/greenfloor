"""Rust-backed transient retry and polling backoff policy."""

from __future__ import annotations

from greenfloor.core.kernel_bridge import import_kernel


def parse_rate_limit_retry_seconds(error_text: str) -> float | None:
    value = import_kernel().parse_rate_limit_retry_seconds(error_text)
    return None if value is None else float(value)


def moderate_retry_sleep_seconds(
    *,
    attempt: int,
    current_sleep: float,
    rate_limit_wait: float | None,
) -> float:
    return float(
        import_kernel().moderate_retry_sleep_seconds(
            int(attempt),
            float(current_sleep),
            rate_limit_wait,
        )
    )


def moderate_retry_next_sleep(current_sleep: float) -> float:
    return float(import_kernel().moderate_retry_next_sleep(float(current_sleep)))


def dexie_invalid_offer_should_retry(*, error: str, attempt: int, max_attempts: int) -> bool:
    return bool(
        import_kernel().dexie_invalid_offer_should_retry(
            str(error),
            int(attempt),
            int(max_attempts),
        )
    )


def dexie_invalid_offer_retry_sleep(*, attempt: int, initial_sleep: float) -> float:
    return float(
        import_kernel().dexie_invalid_offer_retry_sleep(int(attempt), float(initial_sleep))
    )


def coinset_fee_lookup_retry_sleep(attempt: int) -> float:
    return float(import_kernel().coinset_fee_lookup_retry_sleep(int(attempt)))


def poll_exponential_next_sleep(
    *,
    elapsed_seconds: int,
    timeout_seconds: int,
    current_sleep: float,
    initial_sleep: float,
    max_sleep: float,
    multiplier: float,
) -> float | None:
    value = import_kernel().poll_exponential_next_sleep(
        int(elapsed_seconds),
        int(timeout_seconds),
        float(current_sleep),
        float(initial_sleep),
        float(max_sleep),
        float(multiplier),
    )
    return None if value is None else float(value)
