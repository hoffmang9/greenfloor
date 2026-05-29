"""Compatibility shim for the renamed engine bridge.

New code should import from :mod:`greenfloor.core.engine_bridge`.
"""

from greenfloor.core.engine_bridge import (
    ENGINE_MODULE as KERNEL_MODULE,
)
from greenfloor.core.engine_bridge import (
    bootstrap_engine,
    coin_ops_engine,
    engine_method_getter,
    engine_rebuild_hint,
    import_engine,
    import_signer,
    policy_engine,
    require_engine_method,
)

import_kernel = import_engine
policy_kernel = policy_engine
coin_ops_kernel = coin_ops_engine
bootstrap_kernel = bootstrap_engine
kernel_method_getter = engine_method_getter
kernel_rebuild_hint = engine_rebuild_hint
require_kernel_method = require_engine_method

__all__ = [
    "KERNEL_MODULE",
    "bootstrap_kernel",
    "coin_ops_kernel",
    "import_kernel",
    "import_signer",
    "kernel_method_getter",
    "kernel_rebuild_hint",
    "policy_kernel",
    "require_kernel_method",
]
