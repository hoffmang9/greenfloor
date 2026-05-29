"""Deprecated PyO3 import path (ADR 0010). Prefer ``greenfloor_kernel`` or ``kernel_bridge``."""

from __future__ import annotations

import importlib
import sys
from types import ModuleType

_kernel = importlib.import_module("greenfloor_kernel")
_shim = sys.modules[__name__]
if isinstance(_shim, ModuleType):
    _shim.__dict__.update(
        {name: value for name, value in vars(_kernel).items() if not name.startswith("_")}
    )
