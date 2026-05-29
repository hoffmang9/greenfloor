"""Shared PyO3 bridge for the Rust deterministic policy kernel.

The compiled extension is still named ``greenfloor_signer`` (see ADR 0010). Python
callers should use :func:`import_kernel`; ``import_signer`` remains as a migration alias.

Deterministic policy bridges use :func:`policy_kernel` with
``PolicyKernelProtocol`` (cycle, cancel, notification, offer, retry, coin-ops).
Coin-operation bridges call :func:`coin_ops_kernel`; adapters and signing paths
call ``import_kernel()`` directly.
"""

from __future__ import annotations

import importlib
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from greenfloor.core.coin_ops.kernel_protocol import CoinOpsKernelProtocol
    from greenfloor.core.kernel_protocol import BootstrapKernelProtocol, PolicyKernelProtocol

_KERNEL_MODULE = "greenfloor_signer"
_INSTALL_HINT = (
    "Install the greenfloor_signer extension (for example: "
    "`maturin develop --manifest-path greenfloor-signer-pyo3/Cargo.toml` from the repo root)."
)


def import_kernel() -> Any:
    try:
        return importlib.import_module(_KERNEL_MODULE)
    except ImportError as exc:
        raise ImportError(
            f"{_KERNEL_MODULE} is not available. {_INSTALL_HINT} Original error: {exc}"
        ) from exc


def policy_kernel() -> PolicyKernelProtocol:
    return import_kernel()  # type: ignore[return-value]


def coin_ops_kernel() -> CoinOpsKernelProtocol:
    return policy_kernel()  # type: ignore[return-value]


def bootstrap_kernel() -> BootstrapKernelProtocol:
    return import_kernel()  # type: ignore[return-value]


# Migration alias — prefer import_kernel for new code.
import_signer = import_kernel
