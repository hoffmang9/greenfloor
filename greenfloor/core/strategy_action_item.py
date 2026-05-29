"""Typed strategy offer action outcome (shared by cycle kernel and daemon dispatch)."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from greenfloor.core.planned_action import PlannedAction, planned_action_side

# Statuses that count toward strategy executed_count, coin-op sell adjustment,
# and reservation release_success.
_COUNTS_AS_EXECUTED_STATUSES = frozenset({"executed", "pending_visibility"})
_MANAGED_POST_SUCCESS_REASON = "managed_offer_post_success"


@dataclass(frozen=True, slots=True)
class StrategyActionItem:
    """Single offer action outcome from strategy dispatch.

    Use ``counts_as_executed`` for strategy metrics, coin-op sell counting, and
    parallel reservation release. Use ``is_managed_post_success`` only for managed
    signer health tracking (excludes local builder successes).
    """

    size: int
    side: str
    status: str
    reason: str
    offer_id: str | None = None
    transient_upstream: bool = False
    extra: dict[str, Any] = field(default_factory=dict)

    def to_audit_dict(self) -> dict[str, Any]:
        payload: dict[str, Any] = {
            "size": self.size,
            "side": self.side,
            "status": self.status,
            "reason": self.reason,
            "offer_id": self.offer_id,
        }
        if self.transient_upstream:
            payload["transient_upstream"] = True
        payload.update(self.extra)
        return payload

    def with_extra(self, **kwargs: Any) -> StrategyActionItem:
        if not kwargs:
            return self
        return StrategyActionItem(
            size=self.size,
            side=self.side,
            status=self.status,
            reason=self.reason,
            offer_id=self.offer_id,
            transient_upstream=self.transient_upstream,
            extra={**self.extra, **kwargs},
        )

    @property
    def normalized_status(self) -> str:
        return self.status.strip().lower()

    @property
    def counts_as_executed(self) -> bool:
        return self.normalized_status in _COUNTS_AS_EXECUTED_STATUSES

    @property
    def is_managed_post_success(self) -> bool:
        return self.counts_as_executed and self.reason.strip() == _MANAGED_POST_SUCCESS_REASON

    @classmethod
    def from_action(
        cls,
        action: Any,
        *,
        status: str,
        reason: str,
        side: str,
        offer_id: str | None = None,
        transient_upstream: bool = False,
        **extra: Any,
    ) -> StrategyActionItem:
        return cls(
            size=int(getattr(action, "size", 0)),
            side=side,
            status=status,
            reason=reason,
            offer_id=offer_id,
            transient_upstream=transient_upstream,
            extra=dict(extra),
        )

    @classmethod
    def from_worker_error(
        cls,
        *,
        action: Any,
        exc: Exception,
        transient_upstream: bool,
    ) -> StrategyActionItem:
        return cls(
            size=int(getattr(action, "size", 0)),
            side=(
                planned_action_side(action)
                if isinstance(action, PlannedAction)
                else str(getattr(action, "side", "sell"))
            ),
            status="skipped",
            reason=f"parallel_offer_worker_error:{exc}",
            offer_id=None,
            transient_upstream=transient_upstream,
        )
