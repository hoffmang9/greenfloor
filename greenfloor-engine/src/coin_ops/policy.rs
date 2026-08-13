use crate::coinset::is_canonical_xch_asset;

#[must_use]
pub fn coin_op_min_amount_mojos(canonical_asset_id: &str) -> i64 {
    if is_canonical_xch_asset(canonical_asset_id) {
        0
    } else {
        1000
    }
}

/// Core threshold check shared by coin selection and target-amount validation.
#[must_use]
pub fn amount_meets_coin_op_min_mojos(amount_mojos: i64, canonical_asset_id: &str) -> bool {
    amount_mojos >= coin_op_min_amount_mojos(canonical_asset_id)
}

#[must_use]
pub fn coin_op_target_amount_allowed(amount_mojos: i64, canonical_asset_id: &str) -> bool {
    amount_meets_coin_op_min_mojos(amount_mojos, canonical_asset_id)
}

/// True when a positive CAT change remainder is below [`coin_op_min_amount_mojos`].
///
/// `remainder_mojos` must already be in on-chain mojos — not ladder base units.
#[must_use]
pub fn cat_overshoot_change_would_be_dust(remainder_mojos: i64, canonical_asset_id: &str) -> bool {
    let min_cat_mojos = coin_op_min_amount_mojos(canonical_asset_id);
    min_cat_mojos > 0 && remainder_mojos > 0 && remainder_mojos < min_cat_mojos
}

/// True when a positive overshoot amount, scaled to mojos, would be CAT dust.
///
/// `amount_to_mojo_multiplier` converts `overshoot_amount` into on-chain mojos (`1` when
/// amounts are already mojos). Non-positive overshoot is never dust.
#[must_use]
pub fn overshoot_change_would_be_dust(
    overshoot_amount: i64,
    amount_to_mojo_multiplier: i64,
    canonical_asset_id: &str,
) -> bool {
    if overshoot_amount <= 0 {
        return false;
    }
    cat_overshoot_change_would_be_dust(
        overshoot_amount.saturating_mul(amount_to_mojo_multiplier.max(1)),
        canonical_asset_id,
    )
}

/// CAT dust filter for covering-set selection (plan units or mojos).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DustChangeFilter<'a> {
    pub mojo_multiplier: i64,
    pub canonical_asset_id: &'a str,
}

impl DustChangeFilter<'_> {
    #[must_use]
    pub(crate) fn change_is_dust(self, overshoot_amount: i64) -> bool {
        overshoot_change_would_be_dust(
            overshoot_amount,
            self.mojo_multiplier,
            self.canonical_asset_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        amount_meets_coin_op_min_mojos, cat_overshoot_change_would_be_dust,
        coin_op_min_amount_mojos, coin_op_target_amount_allowed, overshoot_change_would_be_dust,
    };
    use crate::coinset::is_canonical_xch_asset;

    const TEST_CAT_ASSET_ID: &str =
        "0000000000000000000000000000000000000000000000000000000000000001";

    #[test]
    fn canonical_xch_matches_python_symbols() {
        assert!(is_canonical_xch_asset("xch"));
        assert!(is_canonical_xch_asset("TXCH"));
        assert!(is_canonical_xch_asset("1"));
        assert!(!is_canonical_xch_asset(""));
        assert!(!is_canonical_xch_asset("  "));
    }

    #[test]
    fn min_amount_guard_for_cat_dust() {
        assert_eq!(coin_op_min_amount_mojos(TEST_CAT_ASSET_ID), 1000);
        assert!(!amount_meets_coin_op_min_mojos(500, TEST_CAT_ASSET_ID));
        assert!(amount_meets_coin_op_min_mojos(1000, TEST_CAT_ASSET_ID));
        assert!(coin_op_target_amount_allowed(1000, TEST_CAT_ASSET_ID));
    }

    #[test]
    fn xch_has_no_min_amount() {
        assert_eq!(coin_op_min_amount_mojos("xch"), 0);
        assert!(amount_meets_coin_op_min_mojos(1, "xch"));
    }

    #[test]
    fn cat_overshoot_dust_rejects_sub_minimum_mojo_remainder() {
        assert!(cat_overshoot_change_would_be_dust(5, TEST_CAT_ASSET_ID));
        assert!(!cat_overshoot_change_would_be_dust(
            5_000,
            TEST_CAT_ASSET_ID
        ));
        assert!(!cat_overshoot_change_would_be_dust(0, TEST_CAT_ASSET_ID));
    }

    #[test]
    fn overshoot_change_scales_by_mojo_multiplier() {
        // 5 plan units × 1000 = 5000 mojos — above the CAT floor.
        assert!(!overshoot_change_would_be_dust(5, 1_000, TEST_CAT_ASSET_ID));
        // 5 mojos (multiplier 1) — dust.
        assert!(overshoot_change_would_be_dust(5, 1, TEST_CAT_ASSET_ID));
        assert!(!overshoot_change_would_be_dust(0, 1_000, TEST_CAT_ASSET_ID));
        assert!(!overshoot_change_would_be_dust(5, 1_000, "xch"));
    }
}
