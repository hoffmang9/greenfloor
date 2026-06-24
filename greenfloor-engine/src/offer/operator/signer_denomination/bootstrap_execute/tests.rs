use crate::config::ManagerProgramConfig;
use crate::offer::bootstrap::{
    plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapPlanOutcome, PlannerLadderRow,
};
use crate::test_support::signer_config::test_signer_config;

use super::super::test_overrides::{
    sample_vault_mixed_split_stub, SignerDenominationTestOverrides,
};
use super::{execute_bootstrap_shape, replan_after_combine, BootstrapShapeContext};

const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
const MOJO_PER_UNIT: u64 = 1_000;
const MOJO_PER_XCH: u64 = 1_000_000_000_000;

fn combine_first_shape_context(
    receive_address: &str,
    split_asset_id: &str,
    ladder: Vec<PlannerLadderRow>,
) -> BootstrapShapeContext {
    let spendable = vec![
        BootstrapCoin {
            id: "sixty-five".to_string(),
            amount: 65,
        },
        BootstrapCoin {
            id: "twenty".to_string(),
            amount: 20,
        },
        BootstrapCoin {
            id: "eleven".to_string(),
            amount: 11,
        },
        BootstrapCoin {
            id: "four".to_string(),
            amount: 4,
        },
    ];
    let BootstrapPlanOutcome::NeedsShape(bootstrap_plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &spendable,
        5,
        &crate::offer::bootstrap::BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected combine-first plan");
    };
    BootstrapShapeContext {
        split_asset_id: split_asset_id.to_string(),
        split_asset_mojo_multiplier: 1_000,
        receive_address: receive_address.to_string(),
        bootstrap_plan,
        ladder_entries: ladder,
        fee_mojos: 0,
        fee_source: String::new(),
        fee_lookup_error: None,
        existing_coin_ids: spendable.iter().map(|coin| coin.id.clone()).collect(),
        test_overrides: SignerDenominationTestOverrides::default(),
    }
}

fn coin_record_body(parent: &str, amount: u64) -> String {
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

fn coin_records_response(records: &[String]) -> String {
    format!(
        r#"{{
            "success": true,
            "coin_records": [{}]
        }}"#,
        records.join(",")
    )
}

#[tokio::test]
async fn replan_after_combine_transitions_to_single_coin_split() {
    let combined_parent = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let combined_record = coin_record_body(combined_parent, MOJO_PER_UNIT * 100);

    let mut server = mockito::Server::new_async().await;
    let combined_coin_body = coin_records_response(&[combined_record]);
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(combined_coin_body)
        .create_async()
        .await;

    let program = ManagerProgramConfig::default();
    let signer = test_signer_config(&server.url());
    let ladder = vec![PlannerLadderRow {
        size_base_units: 100,
        target_count: 1,
        split_buffer_count: 0,
    }];
    let mut ctx = combine_first_shape_context(RECEIVE_ADDRESS, "xch", ladder);
    assert!(ctx.bootstrap_plan.requires_combine_first());

    let replanned = replan_after_combine(
        &program,
        &signer,
        &mut ctx,
        vec![serde_json::json!({"event": "bootstrap_combine_submitted"})],
    )
    .await
    .expect("replan");

    match replanned {
        None => {
            assert!(!ctx.bootstrap_plan.requires_combine_first());
            assert_eq!(ctx.bootstrap_plan.output_amounts_base_units, vec![100]);
        }
        Some(result) => {
            assert!(result.ready);
            assert_eq!(result.reason, "bootstrap_submitted");
        }
    }
}

#[tokio::test]
async fn prepare_and_replan_combine_first_inventory() {
    use crate::offer::operator::signer_denomination::prepare_bootstrap_execution_plan;
    use crate::test_support::ladder::market_with_side_ladder;

    let mut server = mockito::Server::new_async().await;
    let fragmented = coin_records_response(&[
        coin_record_body(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            MOJO_PER_XCH * 65,
        ),
        coin_record_body(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            MOJO_PER_XCH * 20,
        ),
        coin_record_body(
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            MOJO_PER_XCH * 11,
        ),
        coin_record_body(
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            MOJO_PER_XCH * 4,
        ),
    ]);
    let combined = coin_records_response(&[coin_record_body(
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        MOJO_PER_XCH * 100,
    )]);
    let _initial = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(fragmented)
        .expect_at_least(1)
        .create_async()
        .await;
    let _after_combine = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(combined)
        .expect_at_least(1)
        .create_async()
        .await;
    let _fee = server
        .mock("POST", "/get_fee_estimate")
        .with_status(200)
        .with_body(r#"{"success":false}"#)
        .create_async()
        .await;

    let mut market = market_with_side_ladder(RECEIVE_ADDRESS, "sell", 100, 1);
    market.ladders.get_mut("sell").expect("sell ladder")[0].split_buffer_count = 0;
    let program = ManagerProgramConfig {
        coin_ops_minimum_fee_mojos: 0,
        ..Default::default()
    };
    let signer = test_signer_config(&server.url());

    let mut shape_ctx =
        prepare_bootstrap_execution_plan(&program, &signer, &market, "sell", "xch", "xch", 1.0)
            .await
            .expect("plan result")
            .expect("shape context");
    assert!(shape_ctx.bootstrap_plan.requires_combine_first());

    let replanned = replan_after_combine(&program, &signer, &mut shape_ctx, Vec::new())
        .await
        .expect("replan");
    match replanned {
        None => {
            assert!(!shape_ctx.bootstrap_plan.requires_combine_first());
            assert_eq!(
                shape_ctx.bootstrap_plan.output_amounts_base_units,
                vec![100]
            );
        }
        Some(result) => {
            assert!(result.ready);
            assert_eq!(result.reason, "bootstrap_submitted");
        }
    }
}

async fn coinset_server_for_combine_first_e2e() -> mockito::ServerGuard {
    let fragmented = coin_records_response(&[
        coin_record_body(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            MOJO_PER_XCH * 65,
        ),
        coin_record_body(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            MOJO_PER_XCH * 20,
        ),
        coin_record_body(
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            MOJO_PER_XCH * 11,
        ),
        coin_record_body(
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            MOJO_PER_XCH * 4,
        ),
    ]);
    let combined_for_wait = coin_records_response(&[
        coin_record_body(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            MOJO_PER_XCH * 65,
        ),
        coin_record_body(
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            MOJO_PER_XCH * 100,
        ),
    ]);
    let combined_only = coin_records_response(&[coin_record_body(
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        MOJO_PER_XCH * 100,
    )]);
    let shaped_for_wait = coin_records_response(&[
        coin_record_body(
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            MOJO_PER_XCH * 100,
        ),
        coin_record_body(
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            MOJO_PER_XCH * 100,
        ),
    ]);
    let shaped_only = coin_records_response(&[coin_record_body(
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        MOJO_PER_XCH * 100,
    )]);

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
        .with_body(combined_for_wait)
        .create_async()
        .await;
    let _replan_refresh = server
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
    let _final_refresh = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(shaped_only)
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

#[tokio::test]
async fn execute_bootstrap_shape_runs_combine_then_split() {
    use crate::offer::operator::signer_denomination::prepare_bootstrap_execution_plan;
    use crate::test_support::ladder::market_with_side_ladder;

    let server = coinset_server_for_combine_first_e2e().await;
    let mut market = market_with_side_ladder(RECEIVE_ADDRESS, "sell", 100, 1);
    market.ladders.get_mut("sell").expect("sell ladder")[0].split_buffer_count = 0;
    let program = ManagerProgramConfig {
        coin_ops_minimum_fee_mojos: 0,
        runtime_offer_bootstrap_wait_timeout_seconds: 30,
        ..Default::default()
    };
    let signer = test_signer_config(&server.url());

    let shape_ctx =
        prepare_bootstrap_execution_plan(&program, &signer, &market, "sell", "xch", "xch", 1.0)
            .await
            .expect("plan result")
            .expect("shape context");
    assert!(shape_ctx.bootstrap_plan.requires_combine_first());
    shape_ctx
        .test_overrides
        .enqueue_vault_mixed_split_stub(sample_vault_mixed_split_stub());
    shape_ctx
        .test_overrides
        .enqueue_vault_mixed_split_stub(sample_vault_mixed_split_stub());

    let result = Box::pin(execute_bootstrap_shape(&program, &signer, shape_ctx))
        .await
        .expect("execute shape");

    assert!(result.ready);
    assert_eq!(result.reason, "bootstrap_submitted");
    assert!(result.wait_events.iter().any(|event| {
        event.get("event") == Some(&serde_json::json!("bootstrap_combine_submitted"))
    }));
    assert!(result
        .wait_events
        .iter()
        .any(|event| event.get("event") == Some(&serde_json::json!("confirmed"))));
    assert!(!result.split_result.is_null());
}
