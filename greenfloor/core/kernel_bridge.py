"""Shared PyO3 bridge for the Rust deterministic policy kernel.

The compiled extension is still named ``greenfloor_signer`` (see ADR 0010). Python
callers should use :func:`import_kernel`; ``import_signer`` remains as a migration alias.

:func:`import_kernel` tries :data:`KERNEL_MODULE_LEGACY` first, then
:data:`KERNEL_MODULE_TARGET`, so the ADR 0010 rename can flip module names without
touching bridge call sites.

``policy_kernel``, ``coin_ops_kernel``, and ``bootstrap_kernel`` are typed views of
the same PyO3 module — use the name that matches the ``Protocol`` at the call site.

Deterministic policy bridges use :func:`require_kernel_method` with
``PolicyKernelProtocol`` symbols. Adapters and signing paths call
``import_kernel()`` directly.
"""

from __future__ import annotations

import importlib
from typing import TYPE_CHECKING, Any, cast

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
    "require_kernel_method",
]


def kernel_rebuild_hint(*, module: str, missing: str = "required kernel") -> str:
    """Operator-facing rebuild message for stale or incomplete PyO3 wheels."""
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


def _kernel_module_label(kernel: Any) -> str:
    name = getattr(kernel, "__name__", None)
    if isinstance(name, str) and name:
        return name
    return KERNEL_MODULE_LEGACY


def require_kernel_method(kernel: Any, method_name: str, *, missing: str) -> Any:
    method = getattr(kernel, method_name, None)
    if method is None:
        raise RuntimeError(
            f"{kernel_rebuild_hint(module=_kernel_module_label(kernel), missing=missing)} "
            f"Missing symbol: {method_name}"
        )
    return method


def policy_kernel() -> PolicyKernelProtocol:
    return cast(Any, import_kernel())


def coin_ops_kernel() -> CoinOpsKernelProtocol:
    return cast(Any, import_kernel())


def bootstrap_kernel() -> BootstrapKernelProtocol:
    return cast(Any, import_kernel())


# Migration alias — prefer import_kernel for new code.
import_signer = import_kernel
