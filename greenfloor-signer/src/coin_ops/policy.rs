/// Match Python ``canonical_is_xch`` (empty/whitespace is not XCH).
pub fn is_canonical_xch_asset(asset_id: &str) -> bool {
    matches!(
        asset_id.trim().to_ascii_lowercase().as_str(),
        "xch" | "txch" | "1"
    )
}

pub fn coin_op_min_amount_mojos(canonical_asset_id: &str) -> i64 {
    if is_canonical_xch_asset(canonical_asset_id) {
        0
    } else {
        1000
    }
}

pub fn coin_meets_coin_op_min_amount(amount_mojos: i64, canonical_asset_id: &str) -> bool {
    amount_mojos >= coin_op_min_amount_mojos(canonical_asset_id)
}

pub fn coin_op_target_amount_allowed(amount_mojos: i64, canonical_asset_id: &str) -> bool {
    amount_mojos >= coin_op_min_amount_mojos(canonical_asset_id)
}

#[cfg(test)]
mod tests {
    use super::{
        coin_meets_coin_op_min_amount, coin_op_min_amount_mojos, coin_op_target_amount_allowed,
        is_canonical_xch_asset,
    };

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
        assert!(!coin_meets_coin_op_min_amount(500, cat_id));
        assert!(coin_meets_coin_op_min_amount(1000, cat_id));
        assert!(coin_op_target_amount_allowed(1000, cat_id));
    }

    #[test]
    fn xch_has_no_min_amount() {
        assert_eq!(coin_op_min_amount_mojos("xch"), 0);
        assert!(coin_meets_coin_op_min_amount(1, "xch"));
    }
}
