"""Composed PyO3 protocol surfaces for the deterministic policy kernel."""

from __future__ import annotations

from typing import Protocol

from greenfloor.core.cancel_kernel_protocol import CancelPolicyKernelProtocol
from greenfloor.core.coin_ops.kernel_protocol import CoinOpsKernelProtocol
from greenfloor.core.cycle_kernel_protocol import CycleKernelProtocol
from greenfloor.core.notification_kernel_protocol import NotificationKernelProtocol
from greenfloor.core.offer_kernel_protocol import OfferPolicyKernelProtocol
from greenfloor.core.retry_kernel_protocol import RetryPolicyKernelProtocol

__all__ = [
    "CancelPolicyKernelProtocol",
    "CoinOpsKernelProtocol",
    "CycleKernelProtocol",
    "DeterministicPolicyKernelProtocol",
    "NotificationKernelProtocol",
    "OfferPolicyKernelProtocol",
    "PolicyKernelProtocol",
    "RetryPolicyKernelProtocol",
]


class DeterministicPolicyKernelProtocol(
    CycleKernelProtocol,
    CancelPolicyKernelProtocol,
    NotificationKernelProtocol,
    Protocol,
):
    """Cycle, cancel, and notification deterministic policy bindings."""


class PolicyKernelProtocol(
    DeterministicPolicyKernelProtocol,
    CoinOpsKernelProtocol,
    OfferPolicyKernelProtocol,
    RetryPolicyKernelProtocol,
    Protocol,
):
    """Full in-process deterministic policy kernel (cycle, coin-ops, offer, retry)."""
