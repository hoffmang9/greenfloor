"""Market dispatch cursor state for daemon cycles (selection policy lives in Rust)."""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass, field


@dataclass(slots=True)
class MarketDispatchState:
    cursor: int = 0
    immediate_requeue_ids: deque[str] = field(default_factory=deque)
