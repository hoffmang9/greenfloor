"""Daemon strategy action dispatch (routing glue only).

Tests that stub managed/local offer IO should monkeypatch ``greenfloor.daemon.offer_dispatch``,
not this package.
"""

from __future__ import annotations

from greenfloor.daemon.strategy_dispatch.dispatch_router import (
    StrategyDispatchMode,
    execute_strategy_dispatch,
    resolve_strategy_dispatch_mode,
)
from greenfloor.daemon.strategy_execution import (
    StrategyActionResult,
    StrategyDispatchHooks,
    hooks_from_module,
)

__all__ = [
    "StrategyActionResult",
    "StrategyDispatchHooks",
    "StrategyDispatchMode",
    "execute_strategy_dispatch",
    "hooks_from_module",
    "resolve_strategy_dispatch_mode",
]
