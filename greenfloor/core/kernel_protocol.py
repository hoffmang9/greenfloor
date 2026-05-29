"""Compatibility shim for engine protocol names.

New code should import from :mod:`greenfloor.core.engine_protocol`.
"""

from greenfloor.core.engine_protocol import (
    BootstrapEngineProtocol,
    CancelPolicyEngineProtocol,
    CoinOpsEngineProtocol,
    CycleEngineProtocol,
    DeterministicPolicyEngineProtocol,
    NotificationEngineProtocol,
    OfferPolicyEngineProtocol,
    PolicyEngineProtocol,
    RetryPolicyEngineProtocol,
)

BootstrapKernelProtocol = BootstrapEngineProtocol
CancelPolicyKernelProtocol = CancelPolicyEngineProtocol
CoinOpsKernelProtocol = CoinOpsEngineProtocol
CycleKernelProtocol = CycleEngineProtocol
DeterministicPolicyKernelProtocol = DeterministicPolicyEngineProtocol
NotificationKernelProtocol = NotificationEngineProtocol
OfferPolicyKernelProtocol = OfferPolicyEngineProtocol
PolicyKernelProtocol = PolicyEngineProtocol
RetryPolicyKernelProtocol = RetryPolicyEngineProtocol

__all__ = [
    "BootstrapKernelProtocol",
    "CancelPolicyKernelProtocol",
    "CoinOpsKernelProtocol",
    "CycleKernelProtocol",
    "DeterministicPolicyKernelProtocol",
    "NotificationKernelProtocol",
    "OfferPolicyKernelProtocol",
    "PolicyKernelProtocol",
    "RetryPolicyKernelProtocol",
]
