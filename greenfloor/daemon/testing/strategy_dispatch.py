"""Managed/local offer dispatch patch points for tests."""

from __future__ import annotations

from greenfloor.core.cycle import (
    expand_planned_actions,
    single_input_preferred_skip_reason,
)
from greenfloor.daemon import strategy_dispatch
from greenfloor.daemon.strategy_dispatch import (
    build_offer_for_action,
    execute_managed_action_with_retry,
    execute_single_local_action,
    execute_single_managed_action,
    execute_strategy_dispatch,
    managed_offer_post,
    resolve_signer_offer_asset_ids_for_reservation,
)

__all__ = [
    "build_offer_for_action",
    "execute_managed_action_with_retry",
    "execute_single_local_action",
    "execute_single_managed_action",
    "execute_strategy_dispatch",
    "expand_planned_actions",
    "managed_offer_post",
    "resolve_signer_offer_asset_ids_for_reservation",
    "single_input_preferred_skip_reason",
    "strategy_dispatch",
]
