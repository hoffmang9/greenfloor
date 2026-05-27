"""Shared Cloud Wallet split/combine execution helpers (CLI + daemon)."""

from __future__ import annotations

import os
import time
from collections.abc import Callable
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter


def _env_int(name: str, default: int, *, minimum: int = 0) -> int:
    raw = os.getenv(name, "").strip()
    if not raw:
        return default
    try:
        value = int(raw)
    except ValueError:
        return default
    return max(minimum, value)


def combine_retry_config() -> tuple[int, int]:
    attempts = _env_int("GREENFLOOR_COIN_OPS_COMBINE_MAX_ATTEMPTS", 3, minimum=1)
    backoff_ms = _env_int("GREENFLOOR_COIN_OPS_COMBINE_BACKOFF_MS", 1000, minimum=0)
    return attempts, backoff_ms


def is_cloud_wallet_rate_limited_error(exc: Exception) -> bool:
    text = str(exc).strip().lower()
    return "status not ok: 429" in text or " 429" in text or text.endswith(":429")


def combine_coins_with_retry(
    *,
    cloud_wallet: CloudWalletAdapter,
    combine_kwargs: dict[str, Any],
    max_attempts: int | None = None,
    backoff_ms: int | None = None,
    sleep_fn: Callable[[float], None] = time.sleep,
) -> dict[str, Any]:
    """Call ``combine_coins`` with rate-limit backoff (daemon + future CLI reuse)."""
    if max_attempts is None or backoff_ms is None:
        configured_attempts, configured_backoff_ms = combine_retry_config()
        max_attempts = configured_attempts if max_attempts is None else max_attempts
        backoff_ms = configured_backoff_ms if backoff_ms is None else backoff_ms
    last_exc: Exception | None = None
    for attempt in range(1, int(max_attempts) + 1):
        try:
            return cloud_wallet.combine_coins(**combine_kwargs)
        except Exception as exc:
            last_exc = exc
            if attempt >= int(max_attempts) or not is_cloud_wallet_rate_limited_error(exc):
                raise
            if backoff_ms > 0:
                sleep_fn((int(backoff_ms) * (2 ** (attempt - 1))) / 1000.0)
    if last_exc is not None:
        raise last_exc
    raise RuntimeError("combine_coins_failed_without_exception")
