"""Shared strategy execution contracts (results + injectable hooks for tests)."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from greenfloor.core.strategy_action_item import StrategyActionItem
from greenfloor.runtime.offer_post_request import ManagedOfferPostResult


@dataclass(frozen=True, slots=True)
class StrategyActionResult:
    planned_count: int
    executed_count: int
    action_items: list[StrategyActionItem]

    @property
    def items(self) -> list[dict[str, Any]]:
        return [item.to_audit_dict() for item in self.action_items]

    @classmethod
    def from_items(
        cls,
        *,
        planned_count: int,
        action_items: list[StrategyActionItem],
    ) -> StrategyActionResult:
        items = list(action_items)
        executed_count = sum(1 for item in items if item.counts_as_executed)
        return cls(
            planned_count=planned_count,
            executed_count=executed_count,
            action_items=items,
        )


@dataclass(frozen=True, slots=True)
class StrategyDispatchHooks:
    """Callable bundle for strategy offer execution."""

    resolve_signer_offer_asset_ids_for_reservation: Callable[..., tuple[str, str, str]]
    managed_offer_post: Callable[..., ManagedOfferPostResult]
    execute_single_managed_action: Callable[..., StrategyActionItem]
    execute_managed_action_with_retry: Callable[..., StrategyActionItem]


def hooks_from_module() -> StrategyDispatchHooks:
    from greenfloor.daemon import offer_dispatch as pkg

    return StrategyDispatchHooks(
        resolve_signer_offer_asset_ids_for_reservation=(
            pkg.resolve_signer_offer_asset_ids_for_reservation
        ),
        managed_offer_post=pkg.managed_offer_post,
        execute_single_managed_action=pkg.execute_single_managed_action,
        execute_managed_action_with_retry=pkg.execute_managed_action_with_retry,
    )
