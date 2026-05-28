"""Per-market cycle result accumulator."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any

from greenfloor.daemon.market_logging import _daemon_logger


@dataclass(slots=True)
class MarketCycleResult:
    cycle_errors: int = 0
    strategy_planned: int = 0
    strategy_executed: int = 0
    cancel_triggered: bool = False
    cancel_planned: int = 0
    cancel_executed: int = 0
    immediate_requeue_requested: bool = False
    immediate_requeue_signals: list[str] = field(default_factory=list)

    def record_phase_error(self) -> None:
        self.cycle_errors += 1

    def merge_strategy_execution(self, *, planned: int, executed: int) -> None:
        self.strategy_planned += max(0, int(planned))
        self.strategy_executed += max(0, int(executed))

    def merge_cancel_policy(self, *, triggered: bool, planned: int, executed: int) -> None:
        if triggered:
            self.cancel_triggered = True
        self.cancel_planned += max(0, int(planned))
        self.cancel_executed += max(0, int(executed))


def log_daemon_event(*, level: int, payload: dict[str, Any]) -> None:
    _daemon_logger.log(level, "daemon_event %s", json.dumps(payload, sort_keys=True))
