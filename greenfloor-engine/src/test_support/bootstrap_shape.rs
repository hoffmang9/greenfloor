//! Shared bootstrap shape / coinset fixtures for signer denomination tests.

use crate::offer::bootstrap::{
    plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapCombineContext, BootstrapPlanOutcome,
    PlanAmount, PlannerLadderRow,
};
use crate::offer::operator::{BootstrapShapeContext, SignerDenominationTestOverrides};

pub const BOOTSTRAP_TEST_RECEIVE: &str =
    "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
pub const BOOTSTRAP_TEST_MOJO_PER_UNIT: u64 = 1_000;
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

/// Coinset mock for combine-first then split wait progression.
pub async fn coinset_server_for_combine_first_e2e() -> mockito::ServerGuard {
    let fragmented = coin_records_response(&[
        coin_record_body(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            BOOTSTRAP_TEST_MOJO_PER_XCH * 65,
        ),
        coin_record_body(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            BOOTSTRAP_TEST_MOJO_PER_XCH * 20,
        ),
        coin_record_body(
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            BOOTSTRAP_TEST_MOJO_PER_XCH * 11,
        ),
        coin_record_body(
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            BOOTSTRAP_TEST_MOJO_PER_XCH * 4,
        ),
    ]);
    let combined_only = coin_records_response(&[coin_record_body(
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        BOOTSTRAP_TEST_MOJO_PER_XCH * 100,
    )]);
    let shaped_for_wait = coin_records_response(&[
        coin_record_body(
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            BOOTSTRAP_TEST_MOJO_PER_XCH * 100,
        ),
        coin_record_body(
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            BOOTSTRAP_TEST_MOJO_PER_XCH * 100,
        ),
    ]);

    let mut server = mockito::Server::new_async().await;
    let _initial = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(fragmented)
        .create_async()
        .await;
    let _combine_wait = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(combined_only)
        .create_async()
        .await;
    let _split_wait = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(shaped_for_wait)
        .create_async()
        .await;
    let _fee = server
        .mock("POST", "/get_fee_estimate")
        .with_status(200)
        .with_body(r#"{"success":false}"#)
        .create_async()
        .await;
    server
}

/// Coinset mock for ECO.181 combine-only (ready after combine, no split).
pub async fn coinset_server_for_eco181_combine_only_e2e() -> mockito::ServerGuard {
    use crate::test_support::eco181_bootstrap_inventory::{
        eco181_after_combine_inventory_rows, eco181_bootstrap_inventory_rows,
        eco181_fixture_coin_records,
    };

    let fragmented = eco181_fixture_coin_records(
        &eco181_bootstrap_inventory_rows(),
        BOOTSTRAP_TEST_MOJO_PER_XCH.cast_signed(),
    );
    let after_combine = eco181_fixture_coin_records(
        &eco181_after_combine_inventory_rows(),
        BOOTSTRAP_TEST_MOJO_PER_XCH.cast_signed(),
    );

    let mut server = mockito::Server::new_async().await;
    let _initial = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(fragmented)
        .create_async()
        .await;
    let _combine_wait = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(after_combine)
        .expect_at_least(2)
        .create_async()
        .await;
    let _fee = server
        .mock("POST", "/get_fee_estimate")
        .with_status(200)
        .with_body(r#"{"success":false}"#)
        .create_async()
        .await;
    server
}

/// ECO.181-style inventory (60k + four 10k plan mojos) for cap-aware combine-first
/// bootstrap tests. Amounts are already in plan mojos.
#[must_use]
pub fn eco181_cap_combine_spendable() -> Vec<BootstrapCoin> {
    vec![
        BootstrapCoin {
            id: "sixty".to_string(),
            amount: PlanAmount::new(60_000),
        },
        BootstrapCoin {
            id: "ten-a".to_string(),
            amount: PlanAmount::new(10_000),
        },
        BootstrapCoin {
            id: "ten-b".to_string(),
            amount: PlanAmount::new(10_000),
        },
        BootstrapCoin {
            id: "ten-c".to_string(),
            amount: PlanAmount::new(10_000),
        },
        BootstrapCoin {
            id: "ten-d".to_string(),
            amount: PlanAmount::new(10_000),
        },
    ]
}

#[must_use]
pub fn combine_first_shape_context(
    receive_address: &str,
    split_asset_id: &str,
    combine_context: BootstrapCombineContext,
    ladder: Vec<PlannerLadderRow>,
    spendable: &[BootstrapCoin],
    combine_input_cap: i64,
) -> BootstrapShapeContext {
    let BootstrapPlanOutcome::NeedsShape(bootstrap_plan) =
        plan_bootstrap_mixed_outputs(&ladder, spendable, combine_input_cap, &combine_context)
    else {
        panic!("expected combine-first plan");
    };
    BootstrapShapeContext {
        split_asset_id: split_asset_id.to_string(),
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
        BootstrapCombineContext::mojos("xch"),
        ladder,
        &spendable,
        5,
    )
}
