"""Shared PyO3 bridge for the Rust deterministic policy kernel.

The compiled extension is still named ``greenfloor_signer`` (see ADR 0010). Python
callers should use :func:`import_kernel`; ``import_signer`` remains as a migration alias.

:func:`import_kernel` tries :data:`KERNEL_MODULE_LEGACY` first, then
:data:`KERNEL_MODULE_TARGET`, so the ADR 0010 rename can flip module names without
touching bridge call sites.

Deterministic policy bridges use :func:`policy_kernel` with
``PolicyKernelProtocol`` (cycle, cancel, notification, offer, retry, coin-ops).
Coin-operation bridges call :func:`coin_ops_kernel`; adapters and signing paths
call ``import_kernel()`` directly.
"""

from __future__ import annotations

import importlib
import importlib.util
import sys
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from greenfloor.core.coin_ops.kernel_protocol import CoinOpsKernelProtocol
    from greenfloor.core.kernel_protocol import BootstrapKernelProtocol, PolicyKernelProtocol

# ADR 0010 naming map — legacy until the post-migration rename ships.
KERNEL_MODULE_LEGACY = "greenfloor_signer"
KERNEL_MODULE_TARGET = "greenfloor_kernel"
_KERNEL_MODULE_CANDIDATES = (KERNEL_MODULE_LEGACY, KERNEL_MODULE_TARGET)

_MATURIN_INSTALL = (
    "`maturin develop --manifest-path greenfloor-signer-pyo3/Cargo.toml` from the repo root"
)

__all__ = [
    "KERNEL_MODULE_LEGACY",
    "KERNEL_MODULE_TARGET",
    "bootstrap_kernel",
    "coin_ops_kernel",
    "import_kernel",
    "import_signer",
    "kernel_rebuild_hint",
    "policy_kernel",
    "resolved_kernel_module_name",
]


def resolved_kernel_module_name() -> str:
    """Return the PyO3 module name :func:`import_kernel` would load first."""
    for module_name in _KERNEL_MODULE_CANDIDATES:
        if module_name in sys.modules:
            return module_name
        try:
            spec = importlib.util.find_spec(module_name)
        except (ImportError, ModuleNotFoundError, ValueError):
            continue
        if spec is not None:
            return module_name
    return KERNEL_MODULE_LEGACY


def kernel_rebuild_hint(*, missing: str = "required kernel") -> str:
    """Operator-facing rebuild message for stale or incomplete PyO3 wheels."""
    module = resolved_kernel_module_name()
    return (
        f"{module} extension is missing {missing} symbols. "
        f"Rebuild it (for example: {_MATURIN_INSTALL})."
    )


def import_kernel() -> Any:
    errors: list[str] = []
    for module_name in _KERNEL_MODULE_CANDIDATES:
        try:
            return importlib.import_module(module_name)
        except ImportError as exc:
            errors.append(f"{module_name}: {exc}")
    raise ImportError(
        "Rust kernel extension is not available "
        f"(tried {', '.join(_KERNEL_MODULE_CANDIDATES)}). "
        f"Install via {_MATURIN_INSTALL}. " + "; ".join(errors)
    )


def policy_kernel() -> PolicyKernelProtocol:
    return import_kernel()  # type: ignore[return-value]


def coin_ops_kernel() -> CoinOpsKernelProtocol:
    return policy_kernel()  # type: ignore[return-value]


def bootstrap_kernel() -> BootstrapKernelProtocol:
    return import_kernel()  # type: ignore[return-value]


# Migration alias — prefer import_kernel for new code.
import_signer = import_kernel
