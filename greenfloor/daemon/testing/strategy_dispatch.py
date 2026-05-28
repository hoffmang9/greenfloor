"""Managed/local offer dispatch patch points.

Prefer patching symbols exported here (submodule callables) over
``greenfloor.daemon.strategy_dispatch._*`` package aliases. When tests no longer
use ``_*`` names, ``strategy_dispatch`` can drop the alias layer; see
``greenfloor.daemon.strategy_dispatch`` module docstring for the removal steps.
"""

from __future__ import annotations

import greenfloor.daemon.strategy_dispatch as strategy_dispatch
from greenfloor.core.cycle import (
    expand_planned_actions,
    single_input_preferred_skip_reason,
)
from greenfloor.daemon.strategy_dispatch.local_path import (
    build_offer_for_action,
    execute_single_local_action,
)
from greenfloor.daemon.strategy_dispatch.managed_path import (
    execute_managed_action_with_retry,
    execute_single_managed_action,
    managed_offer_post,
)

execute_strategy_actions = strategy_dispatch._execute_strategy_actions

__all__ = [
    "build_offer_for_action",
    "execute_managed_action_with_retry",
    "execute_single_local_action",
    "execute_single_managed_action",
    "execute_strategy_actions",
    "expand_planned_actions",
    "managed_offer_post",
    "single_input_preferred_skip_reason",
    "strategy_dispatch",
]
