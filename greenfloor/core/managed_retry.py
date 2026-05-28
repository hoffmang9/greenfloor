"""Managed offer post retry decision (Rust FFI)."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class ManagedRetryDecision:
    decision: str
    sleep_ms: int

    @property
    def should_retry(self) -> bool:
        return self.decision == "retry"
