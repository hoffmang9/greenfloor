"""Typed strategy offer action result items."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True, slots=True)
class StrategyActionItem:
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
    def is_executed(self) -> bool:
        return self.status.strip().lower() == "executed"

    @property
    def counts_as_executed(self) -> bool:
        normalized = self.status.strip().lower()
        return normalized in ("executed", "pending_visibility")

    @property
    def is_managed_post_success(self) -> bool:
        """Managed signer post succeeded, including Dexie visibility pending."""
        return self.counts_as_executed and self.reason.strip() == "managed_offer_post_success"

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
        from greenfloor.daemon.market_helpers import _normalize_offer_side

        return cls(
            size=int(getattr(action, "size", 0)),
            side=_normalize_offer_side(getattr(action, "side", "sell")),
            status="skipped",
            reason=f"parallel_offer_worker_error:{exc}",
            offer_id=None,
            transient_upstream=transient_upstream,
        )
