from greenfloor.core.inventory import compute_bucket_counts_from_coins


def test_bucket_counts_exact_matches_only() -> None:
    got = compute_bucket_counts_from_coins(
        coin_amounts_base_units=[1, 1, 2, 10, 100, 99],
        ladder_sizes=[1, 10, 100],
    )
    assert got == {1: 2, 10: 1, 100: 1}
