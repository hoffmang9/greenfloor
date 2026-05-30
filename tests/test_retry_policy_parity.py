"""Parity tests for Rust-backed retry policy."""

from __future__ import annotations

from greenfloor.core.policy_bridge import (
    coinset_fee_lookup_retry_sleep,
    dexie_invalid_offer_retry_sleep,
    dexie_invalid_offer_should_retry,
    parse_rate_limit_retry_seconds,
)
from greenfloor.moderate_retry import call_with_moderate_retry, poll_with_exponential_backoff_until


def test_parse_rate_limit_retry_seconds_parity() -> None:
    assert parse_rate_limit_retry_seconds("try again in 12 seconds") == 12.0
    assert parse_rate_limit_retry_seconds("Try Again In 3 Seconds") == 3.0
    assert parse_rate_limit_retry_seconds("rate limited") is None


def test_dexie_invalid_offer_retry_policy() -> None:
    err = 'dexie_http_error:400:{"error_message":"Invalid Offer"}'
    assert dexie_invalid_offer_should_retry(error=err, attempt=0, max_attempts=4)
    assert not dexie_invalid_offer_should_retry(error=err, attempt=3, max_attempts=4)
    assert dexie_invalid_offer_retry_sleep(attempt=0, initial_sleep=1.0) == 1.0
    assert dexie_invalid_offer_retry_sleep(attempt=2, initial_sleep=1.0) == 4.0


def test_coinset_fee_lookup_retry_sleep_parity() -> None:
    assert coinset_fee_lookup_retry_sleep(0) == 0.5
    assert coinset_fee_lookup_retry_sleep(2) == 2.0


def test_call_with_moderate_retry_uses_engine_backoff() -> None:
    sleeps: list[float] = []
    attempts = {"count": 0}

    def _call():
        attempts["count"] += 1
        if attempts["count"] < 2:
            raise RuntimeError("try again in 5 seconds")
        return "ok"

    result = call_with_moderate_retry(
        action="test",
        call=_call,
        max_attempts=4,
        sleep_fn=lambda s: sleeps.append(s),
    )
    assert result == "ok"
    assert len(sleeps) == 1
    assert sleeps[0] >= 5.25


def test_poll_with_exponential_backoff_until_returns_when_ready() -> None:
    ticks = {"count": 0}

    def _on_tick(_elapsed: int) -> str | None:
        ticks["count"] += 1
        return "done" if ticks["count"] >= 2 else None

    result = poll_with_exponential_backoff_until(
        monotonic_fn=lambda: float(ticks["count"]),
        sleep_fn=lambda _s: None,
        timeout_seconds=10,
        initial_sleep=0.5,
        max_sleep=8.0,
        sleep_multiplier=2.0,
        on_tick=_on_tick,
        timeout_error="timed_out",
    )
    assert result == "done"
