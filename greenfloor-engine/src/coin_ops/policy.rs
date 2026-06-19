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

#[cfg(test)]
mod tests {
    use super::{
        amount_meets_coin_op_min_mojos, coin_op_min_amount_mojos, coin_op_target_amount_allowed,
    };
    use crate::coinset::is_canonical_xch_asset;

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
        let cat_id = "0000000000000000000000000000000000000000000000000000000000000001";
        assert_eq!(coin_op_min_amount_mojos(cat_id), 1000);
        assert!(!amount_meets_coin_op_min_mojos(500, cat_id));
        assert!(amount_meets_coin_op_min_mojos(1000, cat_id));
        assert!(coin_op_target_amount_allowed(1000, cat_id));
    }

    #[test]
    fn xch_has_no_min_amount() {
        assert_eq!(coin_op_min_amount_mojos("xch"), 0);
        assert!(amount_meets_coin_op_min_mojos(1, "xch"));
    }
}
