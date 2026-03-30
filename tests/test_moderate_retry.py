from __future__ import annotations

import pytest

from greenfloor.moderate_retry import (
    call_with_moderate_retry,
    cloud_wallet_rate_limit_retry_seconds,
    poll_with_exponential_backoff_until,
)


def test_cloud_wallet_rate_limit_retry_seconds_parses_value() -> None:
    assert cloud_wallet_rate_limit_retry_seconds("try again in 12 seconds") == 12.0
    assert cloud_wallet_rate_limit_retry_seconds("Try Again In 3 Seconds") == 3.0
    assert cloud_wallet_rate_limit_retry_seconds("rate limited") is None


def test_call_with_moderate_retry_returns_immediately_on_success() -> None:
    sleep_calls: list[float] = []
    attempts = {"count": 0}

    def _call():
        attempts["count"] += 1
        return "ok"

    result = call_with_moderate_retry(
        action="poll_status",
        call=_call,
        sleep_fn=lambda seconds: sleep_calls.append(seconds),
    )

    assert result == "ok"
    assert attempts["count"] == 1
    assert sleep_calls == []


def test_call_with_moderate_retry_retries_then_succeeds() -> None:
    sleep_calls: list[float] = []
    events: list[dict[str, str]] = []
    attempts = {"count": 0}

    def _call():
        attempts["count"] += 1
        if attempts["count"] < 3:
            raise RuntimeError("temporary")
        return "ok"

    result = call_with_moderate_retry(
        action="poll_status",
        call=_call,
        elapsed_seconds=9,
        events=events,
        max_attempts=4,
        sleep_fn=lambda seconds: sleep_calls.append(seconds),
    )

    assert result == "ok"
    assert attempts["count"] == 3
    assert sleep_calls == [0.5, 1.0]
    assert [event["attempt"] for event in events] == ["1", "2"]
    assert all(event["action"] == "poll_status" for event in events)
    assert all(event["elapsed_seconds"] == "9" for event in events)


def test_call_with_moderate_retry_uses_rate_limit_hint_for_wait() -> None:
    sleep_calls: list[float] = []
    attempts = {"count": 0}

    def _call():
        attempts["count"] += 1
        if attempts["count"] == 1:
            raise RuntimeError("rate limit: try again in 5 seconds")
        return "ok"

    result = call_with_moderate_retry(
        action="poll_status",
        call=_call,
        max_attempts=3,
        sleep_fn=lambda seconds: sleep_calls.append(seconds),
    )

    assert result == "ok"
    assert sleep_calls == [5.25]


def test_call_with_moderate_retry_raises_on_exhaustion() -> None:
    sleep_calls: list[float] = []

    def _call():
        raise RuntimeError("still failing")

    with pytest.raises(RuntimeError, match="poll_status_retry_exhausted:still failing"):
        call_with_moderate_retry(
            action="poll_status",
            call=_call,
            max_attempts=3,
            sleep_fn=lambda seconds: sleep_calls.append(seconds),
        )

    assert sleep_calls == [0.5, 1.0]


def test_poll_with_exponential_backoff_until_returns_when_on_tick_produces_result() -> None:
    sleeps: list[float] = []
    elapsed_seen: list[int] = []
    monotonic_values = iter([100.0, 100.0, 101.0, 103.0])

    def _monotonic() -> float:
        return next(monotonic_values)

    def _on_tick(elapsed: int) -> str | None:
        elapsed_seen.append(elapsed)
        if elapsed >= 3:
            return "ready"
        return None

    result = poll_with_exponential_backoff_until(
        monotonic_fn=_monotonic,
        sleep_fn=lambda seconds: sleeps.append(seconds),
        timeout_seconds=10,
        initial_sleep=0.5,
        max_sleep=2.0,
        sleep_multiplier=2.0,
        on_tick=_on_tick,
        timeout_error="timeout",
    )

    assert result == "ready"
    assert elapsed_seen == [0, 1, 3]
    assert sleeps == [0.5, 1.0]


def test_poll_with_exponential_backoff_until_raises_timeout() -> None:
    sleeps: list[float] = []
    monotonic_values = iter([200.0, 200.0, 201.0, 202.0])

    def _monotonic() -> float:
        return next(monotonic_values)

    with pytest.raises(RuntimeError, match="poll_timeout"):
        poll_with_exponential_backoff_until(
            monotonic_fn=_monotonic,
            sleep_fn=lambda seconds: sleeps.append(seconds),
            timeout_seconds=2,
            initial_sleep=0.5,
            max_sleep=8.0,
            sleep_multiplier=2.0,
            on_tick=lambda _elapsed: None,
            timeout_error="poll_timeout",
        )

    assert sleeps == [0.5, 1.0]
