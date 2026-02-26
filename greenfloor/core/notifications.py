from __future__ import annotations

from dataclasses import dataclass
from datetime import UTC, datetime, timedelta

from greenfloor.config.models import MarketConfig, ProgramConfig


@dataclass(frozen=True, slots=True)
class AlertEvent:
    market_id: str
    ticker: str
    remaining_amount: int
    receive_address: str
    reason: str


@dataclass(slots=True)
class AlertState:
    is_low: bool = False
    last_alert_at: datetime | None = None


def compute_low_inventory_threshold(program: ProgramConfig, market: MarketConfig) -> int:
    if market.inventory.low_inventory_alert_threshold_base_units is not None:
        return market.inventory.low_inventory_alert_threshold_base_units
    if program.low_inventory_default_threshold_base_units > 0:
        return program.low_inventory_default_threshold_base_units
    return market.inventory.low_watermark_base_units


def evaluate_low_inventory_alert(
    *,
    now: datetime,
    program: ProgramConfig,
    market: MarketConfig,
    state: AlertState,
) -> tuple[AlertState, AlertEvent | None]:
    if not market.enabled or not program.low_inventory_enabled:
        return state, None

    threshold = compute_low_inventory_threshold(program, market)
    remaining = market.inventory.current_available_base_units
    hysteresis_target = int(threshold * (1 + program.low_inventory_clear_hysteresis_percent / 100))

    next_state = AlertState(is_low=state.is_low, last_alert_at=state.last_alert_at)

    if remaining >= hysteresis_target:
        next_state.is_low = False
        return next_state, None

    if remaining >= threshold:
        return next_state, None

    should_send = False
    reason = "low_triggered"
    if not state.is_low:
        should_send = True
    elif state.last_alert_at is None:
        should_send = True
    else:
        cooldown = timedelta(seconds=program.low_inventory_dedup_cooldown_seconds)
        should_send = now - state.last_alert_at >= cooldown
        reason = "reminder_sent"

    next_state.is_low = True
    if should_send:
        next_state.last_alert_at = now
        event = AlertEvent(
            market_id=market.market_id,
            ticker=market.base_symbol,
            remaining_amount=remaining,
            receive_address=market.receive_address,
            reason=reason,
        )
        return next_state, event
    return next_state, None


def utcnow() -> datetime:
    return datetime.now(UTC)
