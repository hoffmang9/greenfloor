from __future__ import annotations


def compute_bucket_counts_from_coins(
    *,
    coin_amounts_base_units: list[int],
    ladder_sizes: list[int],
) -> dict[int, int]:
    """Compute per-size bucket counts from available coin amounts.

    V1 logic is exact-match by ladder size to keep behavior deterministic and auditable.
    """
    ladder = set(ladder_sizes)
    counts: dict[int, int] = {size: 0 for size in ladder_sizes}
    for amount in coin_amounts_base_units:
        if amount in ladder:
            counts[amount] += 1
    return counts
