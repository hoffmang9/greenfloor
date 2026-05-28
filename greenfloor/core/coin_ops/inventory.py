from __future__ import annotations

from greenfloor.core.coin_ops._bridge import _kernel


def compute_bucket_counts_from_coins(
    *,
    coin_amounts_base_units: list[int],
    ladder_sizes: list[int],
) -> dict[int, int]:
    """Compute per-size bucket counts from available coin amounts.

    V1 logic is exact-match by ladder size to keep behavior deterministic and auditable.
    """
    raw = _kernel().compute_bucket_counts_from_coins(
        [int(amount) for amount in coin_amounts_base_units],
        [int(size) for size in ladder_sizes],
    )
    if not isinstance(raw, dict):
        raise TypeError("kernel returned non-dict result")
    return {int(key): int(value) for key, value in raw.items()}
