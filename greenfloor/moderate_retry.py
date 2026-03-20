"""Shared transient-retry and deadline polling helpers for Cloud Wallet / HTTP paths."""

from __future__ import annotations

import collections.abc
import re
import time
from typing import Any, TypeVar

_T = TypeVar("_T")


def cloud_wallet_rate_limit_retry_seconds(error_text: str) -> float | None:
    match = re.search(r"try again in (\d+) seconds", error_text, flags=re.IGNORECASE)
    if not match:
        return None
    try:
        return float(int(match.group(1)))
    except (TypeError, ValueError):
        return None


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
            rate_limit_wait = cloud_wallet_rate_limit_retry_seconds(error_text)
            if rate_limit_wait is not None:
                sleep_seconds = max(sleep_seconds, min(30.0, rate_limit_wait + 0.25))
            if attempt >= max_attempts:
                raise RuntimeError(f"{action}_retry_exhausted:{exc}") from exc
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
            sleep_seconds = min(8.0, sleep_seconds * 2.0)


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
        if elapsed >= timeout_seconds:
            raise RuntimeError(timeout_error)
        sleep_fn(sleep_seconds)
        sleep_seconds = min(max_sleep, sleep_seconds * sleep_multiplier)
