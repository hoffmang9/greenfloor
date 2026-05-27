"""Offer post/cancel cooldown state and retry helpers."""

from __future__ import annotations

import os
import threading
import time
from collections import deque
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter

_PENDING_VISIBILITY_REASON = "managed_offer_post_success_dexie_visibility_pending"
PENDING_VISIBILITY_REASON = _PENDING_VISIBILITY_REASON

_POST_COOLDOWN_UNTIL: dict[str, float] = {}
_CANCEL_COOLDOWN_UNTIL: dict[str, float] = {}
_COOLDOWN_LOCK = threading.Lock()


def _env_int(name: str, default: int, minimum: int = 0) -> int:
    raw = os.getenv(name, "").strip()
    if not raw:
        return default
    try:
        value = int(raw)
    except ValueError:
        return default
    return max(minimum, value)


def _post_retry_config() -> tuple[int, int, int]:
    attempts = _env_int("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", 2, minimum=1)
    backoff_ms = _env_int("GREENFLOOR_OFFER_POST_BACKOFF_MS", 250, minimum=0)
    cooldown_seconds = _env_int("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", 30, minimum=0)
    return attempts, backoff_ms, cooldown_seconds


def _cancel_retry_config() -> tuple[int, int, int]:
    attempts = _env_int("GREENFLOOR_OFFER_CANCEL_MAX_ATTEMPTS", 2, minimum=1)
    backoff_ms = _env_int("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", 250, minimum=0)
    cooldown_seconds = _env_int("GREENFLOOR_OFFER_CANCEL_COOLDOWN_SECONDS", 30, minimum=0)
    return attempts, backoff_ms, cooldown_seconds


def _combine_input_coin_cap() -> int:
    # Keep CAT parent lookup fan-out bounded when resolving many combine inputs.
    return _env_int("GREENFLOOR_COIN_OPS_COMBINE_INPUT_COIN_CAP", 5, minimum=2)


def _is_transient_managed_upstream_error_text(error_text: str) -> bool:
    normalized = str(error_text or "").strip().lower()
    transient_markers = (
        "timed out",
        "timeout",
        "temporary unavailable",
        "temporarily unavailable",
        "bad gateway",
        "gateway timeout",
        "service unavailable",
        "connection reset",
        "connection refused",
        "managed_offer_http_error:502",
        "managed_offer_http_error:503",
        "managed_offer_http_error:504",
        "managed_offer_network_error",
        "signer_http_error:502",
        "signer_http_error:503",
        "signer_http_error:504",
    )
    return any(marker in normalized for marker in transient_markers)


class ManagedUpstreamTransientError(Exception):
    """Transient managed-offer or signer upstream failure (timeouts, HTTP 502/503/504)."""


def is_transient_managed_upstream_error(exc: BaseException) -> bool:
    return isinstance(exc, ManagedUpstreamTransientError)


def strategy_action_item_transient_upstream(item: dict[str, Any]) -> bool:
    return item.get("transient_upstream") is True


def is_transient_managed_upstream_reason(reason: str) -> bool:
    normalized = str(reason or "").strip()
    if normalized == "managed_offer_transient_upstream":
        return True
    if normalized.startswith("managed_offer_post_failed:"):
        return _is_transient_managed_upstream_error_text(normalized.split(":", 1)[1])
    if normalized.startswith("parallel_offer_worker_error:"):
        return _is_transient_managed_upstream_error_text(normalized.split(":", 1)[1])
    return _is_transient_managed_upstream_error_text(normalized)


def transient_managed_upstream_error_from_text(
    error_text: str,
) -> ManagedUpstreamTransientError | None:
    if _is_transient_managed_upstream_error_text(error_text):
        return ManagedUpstreamTransientError(error_text)
    return None


def raise_if_transient_managed_upstream_error(error_text: str) -> None:
    transient = transient_managed_upstream_error_from_text(error_text)
    if transient is not None:
        raise transient


def _managed_offer_reason_is_503(reason_text: str) -> bool:
    normalized = str(reason_text or "").strip().lower()
    return (
        "managed_offer_http_error:503" in normalized
        or "503 service temporarily unavailable" in normalized
    )


def _managed_offer_item_is_success(item: dict[str, Any]) -> bool:
    status = str(item.get("status", "")).strip().lower()
    reason = str(item.get("reason", "")).strip().lower()
    return status == "executed" and (
        reason == "managed_offer_post_success" or reason == _PENDING_VISIBILITY_REASON.lower()
    )


def _parse_iso_datetime(value: str) -> datetime | None:
    text = str(value or "").strip()
    if not text:
        return None
    try:
        return datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError:
        return None


@dataclass(slots=True)
class _ManagedOfferHealthSnapshot:
    count_503: int
    had_success: bool
    timestamp: datetime


_MANAGED_OFFER_HEALTH_WINDOW: dict[str, deque[_ManagedOfferHealthSnapshot]] = {}


def _managed_offer_market_health_payload(
    *,
    market_id: str,
    current_items: list[dict[str, Any]],
    now: datetime,
    window_size: int = 40,
) -> dict[str, Any]:
    window = _MANAGED_OFFER_HEALTH_WINDOW.setdefault(
        str(market_id), deque(maxlen=max(1, window_size))
    )
    batch_503 = sum(
        1 for item in current_items if _managed_offer_reason_is_503(str(item.get("reason", "")))
    )
    batch_success = any(_managed_offer_item_is_success(item) for item in current_items)
    window.append(
        _ManagedOfferHealthSnapshot(count_503=batch_503, had_success=batch_success, timestamp=now)
    )

    rolling_503_count = sum(s.count_503 for s in window)
    last_success_at: str | None = None
    for s in reversed(window):
        if s.had_success:
            last_success_at = s.timestamp.isoformat()
            break
    last_success_age_seconds: int | None = None
    if last_success_at is not None:
        parsed = _parse_iso_datetime(last_success_at)
        if parsed is not None:
            if parsed.tzinfo is None:
                parsed = parsed.replace(tzinfo=UTC)
            last_success_age_seconds = max(0, int((now - parsed).total_seconds()))
    return {
        "market_id": str(market_id),
        "rolling_window_events": len(window),
        "rolling_503_count": int(rolling_503_count),
        "last_managed_offer_success_at": last_success_at,
        "last_managed_offer_success_age_seconds": last_success_age_seconds,
    }


def _cooldown_remaining_ms(cooldowns: dict[str, float], key: str) -> int:
    with _COOLDOWN_LOCK:
        deadline = float(cooldowns.get(key, 0.0))
    remaining = max(0.0, deadline - time.monotonic())
    return int(remaining * 1000)


def _set_cooldown(cooldowns: dict[str, float], key: str, cooldown_seconds: int) -> None:
    if cooldown_seconds <= 0:
        return
    with _COOLDOWN_LOCK:
        cooldowns[key] = time.monotonic() + float(cooldown_seconds)


def _retry_with_backoff(
    *,
    action_fn: Callable[[], dict[str, Any]],
    is_success: Callable[[dict[str, Any]], bool],
    default_error: str,
    retry_config: tuple[int, int, int],
) -> tuple[dict[str, Any], int, str]:
    """Generic retry loop with exponential backoff."""
    attempts_max, backoff_ms, _ = retry_config
    last_error = default_error
    for attempt in range(1, attempts_max + 1):
        try:
            result = action_fn()
        except Exception as exc:
            result = {"success": False, "error": f"{default_error}:{exc}"}
        if is_success(result):
            return result, attempt, ""
        last_error = str(result.get("error", default_error))
        if attempt < attempts_max and backoff_ms > 0:
            time.sleep((backoff_ms * (2 ** (attempt - 1))) / 1000.0)
    return {"success": False, "error": last_error}, attempts_max, last_error


def _is_venue_post_success(result: dict[str, Any]) -> bool:
    return bool(result.get("success", False)) and bool(str(result.get("id", "")).strip())


def _is_cancel_success(result: dict[str, Any]) -> bool:
    return bool(result.get("success", False))


def _post_offer_with_retry(
    *,
    publish_venue: str,
    offer_text: str,
    dexie: DexieAdapter,
    splash: SplashAdapter | None,
) -> tuple[dict[str, Any], int, str]:
    def _do_post() -> dict[str, Any]:
        if publish_venue == "splash":
            if splash is None:
                return {"success": False, "error": "splash_not_configured"}
            return splash.post_offer(offer_text)
        return dexie.post_offer(offer_text)

    return _retry_with_backoff(
        action_fn=_do_post,
        is_success=_is_venue_post_success,
        default_error=f"{publish_venue}_post_failed",
        retry_config=_post_retry_config(),
    )


def _cancel_offer_with_retry(
    *,
    dexie: DexieAdapter,
    offer_id: str,
) -> tuple[dict[str, Any], int, str]:
    return _retry_with_backoff(
        action_fn=lambda: dexie.cancel_offer(offer_id),
        is_success=_is_cancel_success,
        default_error="cancel_offer_failed",
        retry_config=_cancel_retry_config(),
    )
