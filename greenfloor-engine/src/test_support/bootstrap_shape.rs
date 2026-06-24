//! Shared bootstrap shape / coinset fixtures for signer denomination tests.

use crate::offer::bootstrap::{
    plan_bootstrap_mixed_outputs, BaseUnits, BootstrapCoin, BootstrapCombineContext,
    BootstrapPlanOutcome, PlannerLadderRow,
};
use crate::offer::operator::{BootstrapShapeContext, SignerDenominationTestOverrides};

pub const BOOTSTRAP_TEST_RECEIVE: &str =
    "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
pub const BOOTSTRAP_TEST_MOJO_PER_UNIT: u64 = 1_000;
pub const BOOTSTRAP_TEST_MOJO_MULTIPLIER: i64 = 1_000;
pub const BOOTSTRAP_TEST_MOJO_PER_XCH: u64 = 1_000_000_000_000;

#[must_use]
pub fn coin_record_body(parent: &str, amount: u64) -> String {
    format!(
        r#"{{
            "coin": {{
                "parent_coin_info": "{parent}",
                "puzzle_hash": "11cd056d9ec93f4612919b445e1ad9afeb7ef7739708c2d16cec4fd2d3cd5e63",
                "amount": {amount}
            }},
            "coinbase": false,
            "confirmed_block_index": 1,
            "spent": false,
            "spent_block_index": 0,
            "timestamp": 1
        }}"#
    )
}

#[must_use]
pub fn coin_records_response(records: &[String]) -> String {
    format!(
        r#"{{
            "success": true,
            "coin_records": [{}]
        }}"#,
        records.join(",")
    )
}

/// Fragmented inventory (65+20+11+4) that requires combine-first for a 100 BU ladder row.
#[must_use]
pub fn fragmented_combine_first_spendable() -> Vec<BootstrapCoin> {
    vec![
        BootstrapCoin {
            id: "sixty-five".to_string(),
            amount: BaseUnits::new(65),
        },
        BootstrapCoin {
            id: "twenty".to_string(),
            amount: BaseUnits::new(20),
        },
        BootstrapCoin {
            id: "eleven".to_string(),
            amount: BaseUnits::new(11),
        },
        BootstrapCoin {
            id: "four".to_string(),
            amount: BaseUnits::new(4),
        },
    ]
}

/// ECO.181-style inventory (60 + four 10 BU) for cap-aware combine-first bootstrap tests.
#[must_use]
pub fn eco181_cap_combine_spendable() -> Vec<BootstrapCoin> {
    vec![
        BootstrapCoin {
            id: "sixty".to_string(),
            amount: BaseUnits::new(60),
        },
        BootstrapCoin {
            id: "ten-a".to_string(),
            amount: BaseUnits::new(10),
        },
        BootstrapCoin {
            id: "ten-b".to_string(),
            amount: BaseUnits::new(10),
        },
        BootstrapCoin {
            id: "ten-c".to_string(),
            amount: BaseUnits::new(10),
        },
        BootstrapCoin {
            id: "ten-d".to_string(),
            amount: BaseUnits::new(10),
        },
    ]
}

#[must_use]
pub fn combine_first_shape_context(
    receive_address: &str,
    split_asset_id: &str,
    mojo_multiplier: i64,
    ladder: Vec<PlannerLadderRow>,
    spendable: &[BootstrapCoin],
    combine_input_cap: i64,
) -> BootstrapShapeContext {
    let combine_context = BootstrapCombineContext::new(mojo_multiplier, split_asset_id);
    let BootstrapPlanOutcome::NeedsShape(bootstrap_plan) =
        plan_bootstrap_mixed_outputs(&ladder, spendable, combine_input_cap, &combine_context)
    else {
        panic!("expected combine-first plan");
    };
    BootstrapShapeContext {
        split_asset_id: split_asset_id.to_string(),
        split_asset_mojo_multiplier: mojo_multiplier,
        receive_address: receive_address.to_string(),
        bootstrap_plan,
        ladder_entries: ladder,
        combine_context,
        fee_mojos: 0,
        fee_source: String::new(),
        fee_lookup_error: None,
        #[cfg(test)]
        test_overrides: SignerDenominationTestOverrides::default(),
    }
}

#[must_use]
pub fn eco181_cap_combine_shape_context(ladder: Vec<PlannerLadderRow>) -> BootstrapShapeContext {
    let spendable = eco181_cap_combine_spendable();
    combine_first_shape_context(
        BOOTSTRAP_TEST_RECEIVE,
        "xch",
        BOOTSTRAP_TEST_MOJO_MULTIPLIER,
        ladder,
        &spendable,
        5,
    )
}

#[must_use]
pub fn fragmented_combine_first_shape_context(
    receive_address: &str,
    split_asset_id: &str,
    ladder: Vec<PlannerLadderRow>,
) -> BootstrapShapeContext {
    let spendable = fragmented_combine_first_spendable();
    combine_first_shape_context(
        receive_address,
        split_asset_id,
        BOOTSTRAP_TEST_MOJO_MULTIPLIER,
        ladder,
        &spendable,
        5,
    )
}
