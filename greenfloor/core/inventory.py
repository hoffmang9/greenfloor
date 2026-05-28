"""Inventory bucket counting (Rust-backed)."""

from __future__ import annotations

from greenfloor.core.coin_ops import _import_signer

__all__ = ["compute_bucket_counts_from_coins"]


def compute_bucket_counts_from_coins(
    *,
    coin_amounts_base_units: list[int],
    ladder_sizes: list[int],
) -> dict[int, int]:
    """Compute per-size bucket counts from available coin amounts.

    V1 logic is exact-match by ladder size to keep behavior deterministic and auditable.
    """
    raw = _import_signer().compute_bucket_counts_from_coins(
        [int(amount) for amount in coin_amounts_base_units],
        [int(size) for size in ladder_sizes],
    )
    if not isinstance(raw, dict):
        raise TypeError("signer returned non-dict result")
    return {int(key): int(value) for key, value in raw.items()}
