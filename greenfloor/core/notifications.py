from __future__ import annotations

from dataclasses import dataclass
from datetime import UTC, datetime

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.kernel_bridge import import_kernel


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


def _datetime_to_unix(value: datetime | None) -> int | None:
    if value is None:
        return None
    return int(value.timestamp())


def _unix_to_datetime(value: int | None) -> datetime | None:
    if value is None:
        return None
    return datetime.fromtimestamp(int(value), tz=UTC)


def evaluate_low_inventory_alert(
    *,
    now: datetime,
    program: ProgramConfig,
    market: MarketConfig,
    state: AlertState,
) -> tuple[AlertState, AlertEvent | None]:
    state_payload, event_payload = import_kernel().evaluate_low_inventory_alert(
        int(now.timestamp()),
        bool(program.low_inventory_enabled),
        int(program.low_inventory_default_threshold_base_units),
        float(program.low_inventory_clear_hysteresis_percent),
        int(program.low_inventory_dedup_cooldown_seconds),
        bool(market.enabled),
        str(market.market_id),
        str(market.base_symbol),
        str(market.receive_address),
        market.inventory.low_inventory_alert_threshold_base_units,
        int(market.inventory.low_watermark_base_units),
        int(market.inventory.current_available_base_units),
        bool(state.is_low),
        _datetime_to_unix(state.last_alert_at),
    )
    next_state = AlertState(
        is_low=bool(state_payload["is_low"]),
        last_alert_at=_unix_to_datetime(state_payload.get("last_alert_at_unix")),
    )
    if event_payload is None:
        return next_state, None
    return next_state, AlertEvent(
        market_id=str(event_payload["market_id"]),
        ticker=str(event_payload["ticker"]),
        remaining_amount=int(event_payload["remaining_amount"]),
        receive_address=str(event_payload["receive_address"]),
        reason=str(event_payload["reason"]),
    )


def utcnow() -> datetime:
    return datetime.now(UTC)
