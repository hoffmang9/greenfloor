"""Daemon strategy action dispatch (routing glue; execution in offer_dispatch)."""

from __future__ import annotations

from greenfloor.daemon.offer_dispatch import (
    build_offer_for_action,
    execute_managed_action_with_retry,
    execute_single_local_action,
    execute_single_managed_action,
    managed_offer_post,
    resolve_signer_offer_asset_ids_for_reservation,
)
from greenfloor.daemon.strategy_dispatch.dispatch_router import (
    StrategyDispatchMode,
    execute_strategy_dispatch,
    resolve_strategy_dispatch_mode,
)
from greenfloor.daemon.strategy_dispatch.results import StrategyActionResult
from greenfloor.daemon.strategy_dispatch.runtime import StrategyDispatchHooks, hooks_from_module

__all__ = [
    "StrategyActionResult",
    "StrategyDispatchHooks",
    "StrategyDispatchMode",
    "build_offer_for_action",
    "execute_managed_action_with_retry",
    "execute_single_local_action",
    "execute_single_managed_action",
    "execute_strategy_dispatch",
    "hooks_from_module",
    "managed_offer_post",
    "resolve_signer_offer_asset_ids_for_reservation",
    "resolve_strategy_dispatch_mode",
]
