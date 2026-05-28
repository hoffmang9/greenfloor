"""Backward-compatible re-export; prefer ``greenfloor.core.coin_ops``."""

from greenfloor.core.coin_ops.inventory import compute_bucket_counts_from_coins

__all__ = ["compute_bucket_counts_from_coins"]
