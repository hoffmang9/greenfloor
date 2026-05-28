"""Shared PyO3 bridge for the Rust deterministic policy kernel.

The compiled extension is still named ``greenfloor_signer`` (see ADR 0010). Python
callers should use :func:`import_kernel`; ``import_signer`` remains as a migration alias.
"""

from __future__ import annotations

import importlib
from typing import Any

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


# Migration alias — prefer import_kernel for new code.
import_signer = import_kernel
