"""Managed offer post / visibility outcome (Rust FFI)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

ManagedActionStatus = Literal["executed", "skipped", "pending_visibility"]


@dataclass(frozen=True, slots=True)
class ManagedActionOutcome:
    status: ManagedActionStatus
    reason: str
    offer_id: str | None
    transient_upstream: bool
    is_pending_visibility: bool
