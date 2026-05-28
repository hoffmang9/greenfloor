from __future__ import annotations

from dataclasses import dataclass
from datetime import UTC, datetime

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.kernel_bridge import policy_kernel


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
    last_alert_at_unix: int | None = None

    @property
    def last_alert_at(self) -> datetime | None:
        return _unix_to_datetime(self.last_alert_at_unix)


@dataclass(frozen=True, slots=True)
class LowInventoryInput:
    now_unix: int
    low_inventory_enabled: bool
    program_default_threshold: int
    clear_hysteresis_percent: float
    dedup_cooldown_seconds: int
    market_enabled: bool
    market_id: str
    ticker: str
    receive_address: str
    market_threshold: int | None
    low_watermark: int
    remaining: int
    state_is_low: bool
    state_last_alert_at_unix: int | None


@dataclass(frozen=True, slots=True)
class LowInventoryEvaluation:
    state: AlertState
    event: AlertEvent | None


def alert_state(
    *,
    is_low: bool = False,
    last_alert_at: datetime | None = None,
) -> AlertState:
    return AlertState(is_low=is_low, last_alert_at_unix=_datetime_to_unix(last_alert_at))


def _datetime_to_unix(value: datetime | None) -> int | None:
    if value is None:
        return None
    return int(value.timestamp())


def _unix_to_datetime(value: int | None) -> datetime | None:
    if value is None:
        return None
    return datetime.fromtimestamp(int(value), tz=UTC)


def _low_inventory_input(
    *,
    now: datetime,
    program: ProgramConfig,
    market: MarketConfig,
    state: AlertState,
) -> LowInventoryInput:
    return LowInventoryInput(
        now_unix=int(now.timestamp()),
        low_inventory_enabled=bool(program.low_inventory_enabled),
        program_default_threshold=int(program.low_inventory_default_threshold_base_units),
        clear_hysteresis_percent=float(program.low_inventory_clear_hysteresis_percent),
        dedup_cooldown_seconds=int(program.low_inventory_dedup_cooldown_seconds),
        market_enabled=bool(market.enabled),
        market_id=str(market.market_id),
        ticker=str(market.base_symbol),
        receive_address=str(market.receive_address),
        market_threshold=market.inventory.low_inventory_alert_threshold_base_units,
        low_watermark=int(market.inventory.low_watermark_base_units),
        remaining=int(market.inventory.current_available_base_units),
        state_is_low=bool(state.is_low),
        state_last_alert_at_unix=state.last_alert_at_unix,
    )


def evaluate_low_inventory_alert(
    *,
    now: datetime,
    program: ProgramConfig,
    market: MarketConfig,
    state: AlertState,
) -> tuple[AlertState, AlertEvent | None]:
    evaluation = policy_kernel().evaluate_low_inventory_alert(
        _low_inventory_input(now=now, program=program, market=market, state=state)
    )
    if not isinstance(evaluation, LowInventoryEvaluation):
        raise TypeError("evaluate_low_inventory_alert returned non-LowInventoryEvaluation result")
    return evaluation.state, evaluation.event


def utcnow() -> datetime:
    return datetime.now(UTC)
