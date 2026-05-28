"""Shared transient-retry and deadline polling helpers (Rust-backed policy)."""

from __future__ import annotations

import collections.abc
import time
from typing import Any, TypeVar

from greenfloor.core.retry_policy import (
    moderate_retry_next_sleep,
    moderate_retry_sleep_seconds,
    parse_rate_limit_retry_seconds,
    poll_exponential_advance_sleep,
    poll_exponential_sleep_now,
)

_T = TypeVar("_T")


def call_with_moderate_retry(
    *,
    action: str,
    call: collections.abc.Callable[[], Any],
    elapsed_seconds: int = 0,
    events: list[dict[str, str]] | None = None,
    max_attempts: int = 4,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
):
    if sleep_fn is None:
        sleep_fn = time.sleep
    attempt = 0
    sleep_seconds = 0.5
    while True:
        try:
            return call()
        except Exception as exc:
            attempt += 1
            error_text = str(exc)
            rate_limit_wait = parse_rate_limit_retry_seconds(error_text)
            if attempt >= max_attempts:
                raise RuntimeError(f"{action}_retry_exhausted:{exc}") from exc
            sleep_seconds = moderate_retry_sleep_seconds(
                current_sleep=sleep_seconds,
                rate_limit_wait=rate_limit_wait,
            )
            if events is not None:
                events.append(
                    {
                        "event": "poll_retry",
                        "action": action,
                        "attempt": str(attempt),
                        "elapsed_seconds": str(elapsed_seconds),
                        "wait_reason": "transient_poll_failure",
                        "error": error_text,
                    }
                )
            sleep_fn(sleep_seconds)
            sleep_seconds = moderate_retry_next_sleep(sleep_seconds)


def poll_with_exponential_backoff_until(
    *,
    monotonic_fn: collections.abc.Callable[[], float],
    sleep_fn: collections.abc.Callable[[float], None],
    timeout_seconds: int,
    initial_sleep: float,
    max_sleep: float,
    sleep_multiplier: float,
    on_tick: collections.abc.Callable[[int], _T | None],
    timeout_error: str,
) -> _T:
    """Call *on_tick(elapsed_seconds)* until it returns non-*None* or timeout."""
    start = monotonic_fn()
    sleep_seconds = initial_sleep
    while True:
        elapsed = int(monotonic_fn() - start)
        result = on_tick(elapsed)
        if result is not None:
            return result
        sleep_now = poll_exponential_sleep_now(
            elapsed_seconds=elapsed,
            timeout_seconds=timeout_seconds,
            sleep_seconds=sleep_seconds,
            initial_sleep=initial_sleep,
            max_sleep=max_sleep,
        )
        if sleep_now is None:
            raise RuntimeError(timeout_error)
        sleep_fn(sleep_now)
        sleep_seconds = poll_exponential_advance_sleep(
            sleep_seconds=sleep_now,
            initial_sleep=initial_sleep,
            max_sleep=max_sleep,
            multiplier=sleep_multiplier,
        )


__all__ = [
    "call_with_moderate_retry",
    "moderate_retry_next_sleep",
    "moderate_retry_sleep_seconds",
    "parse_rate_limit_retry_seconds",
    "poll_exponential_advance_sleep",
    "poll_exponential_sleep_now",
    "poll_with_exponential_backoff_until",
]
