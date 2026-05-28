"""Low-inventory notification PyO3 protocol surface."""

from __future__ import annotations

from typing import TYPE_CHECKING, Protocol

if TYPE_CHECKING:
    from greenfloor.core.notifications import (
        LowInventoryEvaluation,
        LowInventoryInput,
    )


class NotificationKernelProtocol(Protocol):
    def evaluate_low_inventory_alert(self, input: LowInventoryInput) -> LowInventoryEvaluation: ...
