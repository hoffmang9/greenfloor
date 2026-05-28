"""Managed offer post retry decision (Rust FFI)."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class ManagedRetryDecision:
    should_retry: bool
    sleep_ms: int
