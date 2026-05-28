"""Backward-compatible re-export; prefer ``greenfloor.core.coin_ops``.

Deprecated: remove after step 11 (coin-op selection/planning Rust migration) when
call sites import from ``greenfloor.core.coin_ops`` directly.
"""

from greenfloor.core.coin_ops.policy import (
    coin_meets_coin_op_min_amount,
    coin_op_min_amount_mojos,
    coin_op_target_amount_allowed,
)

__all__ = [
    "coin_meets_coin_op_min_amount",
    "coin_op_min_amount_mojos",
    "coin_op_target_amount_allowed",
]
