"""Backward-compatible re-export; prefer ``greenfloor.core.coin_ops``.

Deprecated: remove after step 11 (coin-op selection/planning Rust migration) when
call sites import from ``greenfloor.core.coin_ops`` directly.
"""

from greenfloor.core.coin_ops.inventory import compute_bucket_counts_from_coins

__all__ = ["compute_bucket_counts_from_coins"]
