"""Managed/local offer dispatch patch points."""

from __future__ import annotations

import greenfloor.daemon.strategy_dispatch as strategy_dispatch
from greenfloor.core.cycle import (
    expand_strategy_actions,
    single_input_preferred_skip_reason,
)
from greenfloor.daemon.strategy_dispatch import (
    _build_offer_for_action as build_offer_for_action,
)
from greenfloor.daemon.strategy_dispatch import (
    _execute_single_local_action as execute_single_local_action,
)
from greenfloor.daemon.strategy_dispatch import (
    _execute_strategy_actions as execute_strategy_actions,
)

__all__ = [
    "build_offer_for_action",
    "execute_single_local_action",
    "execute_strategy_actions",
    "expand_strategy_actions",
    "single_input_preferred_skip_reason",
    "strategy_dispatch",
]
