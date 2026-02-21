from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class AlertEvent:
    market_id: str
    ticker: str
    remaining_amount: int
    receive_address: str
    reason: str
