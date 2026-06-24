use crate::coinset::WalletUnspentCoin;
use crate::offer::bootstrap::{
    BootstrapCombineContext, BootstrapFundingSource, BootstrapPlan, PlannerLadderRow,
};

use super::{
    bootstrap_skipped, executed_after_split, run_signer_denomination_phase,
    spendable_bootstrap_coins, ExecutedAfterSplitParams,
};

#[test]
fn spendable_bootstrap_coins_filters_unconfirmed_wallet_rows() {
    let coins = vec![
        WalletUnspentCoin {
            id: "confirmed".to_string(),
            name: "confirmed".to_string(),
            amount: 1000,
            state: "CONFIRMED".to_string(),
        },
        WalletUnspentCoin {
            id: "pending".to_string(),
            name: "pending".to_string(),
            amount: 2000,
            state: "PENDING".to_string(),
        },
    ];

    let spendable = spendable_bootstrap_coins(&coins);
    assert_eq!(spendable.len(), 1);
    assert_eq!(spendable[0].id, "confirmed");
    assert_eq!(spendable[0].amount, 1000);
}

#[test]
fn bootstrap_skipped_marks_phase_not_ready() {
    let result = bootstrap_skipped("missing_sell_ladder");
    assert!(!result.ready);
    assert_eq!(result.reason, "missing_sell_ladder");
    assert!(result.offer_creation_block_error().is_some());
}

#[test]
fn executed_after_split_carries_fee_and_plan_metadata() {
    let bootstrap_plan = BootstrapPlan {
        funding: BootstrapFundingSource::SingleCoin {
            coin_id: "coin-a".to_string(),
            amount: 50_000,
        },
        output_amounts_base_units: vec![100, 100],
        total_output_amount: 200,
        change_amount: 49_800,
        deficits: Vec::new(),
    };
    let ladder_entries = vec![PlannerLadderRow {
        size_base_units: 100,
        target_count: 2,
        split_buffer_count: 0,
    }];
    let refreshed = vec![crate::offer::bootstrap::BootstrapCoin {
        id: "coin-a".to_string(),
        amount: 50_000,
    }];

    let result = executed_after_split(ExecutedAfterSplitParams {
        fee_mojos: 0,
        fee_source: String::new(),
        fee_lookup_error: None,
        split_result: serde_json::json!({"operation_id": "split-1"}),
        wait_events: vec![serde_json::json!({"event": "confirmed"})],
        bootstrap_plan,
        ladder_entries: &ladder_entries,
        refreshed_spendable: &refreshed,
        combine_context: BootstrapCombineContext::for_tests(),
    });

    assert_eq!(result.split_result["operation_id"], "split-1");
    assert_eq!(result.wait_events.len(), 1);
    assert!(result.plan.is_some());
}

#[tokio::test]
async fn run_signer_denomination_phase_skips_missing_receive_address() {
    use crate::config::ManagerProgramConfig;
    use crate::test_support::ladder::market_with_side_ladder;
    use crate::test_support::signer_config::test_signer_config;

    let mut market = market_with_side_ladder("", "sell", 10, 2);
    market.receive_address.clear();
    let program = ManagerProgramConfig::default();
    let signer = test_signer_config("https://example.test");

    let result =
        run_signer_denomination_phase(&program, &market, &signer, "xch", "xch", 1.0, "sell")
            .await
            .expect("phase");

    assert_eq!(result.reason, "missing_receive_address_for_bootstrap");
    assert!(!result.ready);
}

#[tokio::test]
async fn run_signer_denomination_phase_uses_signer_coinset_msp_base_url_for_coin_list() {
    use crate::config::ManagerProgramConfig;
    use crate::test_support::ladder::market_with_side_ladder;
    use crate::test_support::signer_config::test_signer_config;

    const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[]}"#)
        .expect_at_least(1)
        .create_async()
        .await;

    let market = market_with_side_ladder(RECEIVE_ADDRESS, "sell", 10, 2);
    let program = ManagerProgramConfig::default();
    let signer = test_signer_config(&server.url());

    let result =
        run_signer_denomination_phase(&program, &market, &signer, "xch", "xch", 1.0, "sell")
            .await
            .expect("phase");

    assert!(
        result.reason.starts_with("bootstrap_underfunded:"),
        "expected underfunded skip after empty coin list, got {}",
        result.reason
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn run_signer_denomination_phase_skips_missing_sell_ladder() {
    use crate::config::ManagerProgramConfig;
    use crate::test_support::ladder::empty_ladders_market;
    use crate::test_support::signer_config::test_signer_config;

    let market = empty_ladders_market("xch1test");
    let program = ManagerProgramConfig::default();
    let signer = test_signer_config("https://example.test");

    let result =
        run_signer_denomination_phase(&program, &market, &signer, "xch", "xch", 1.0, "sell")
            .await
            .expect("phase");

    assert_eq!(result.reason, "missing_sell_ladder");
}

#[tokio::test]
async fn run_signer_denomination_phase_fails_when_coin_list_errors() {
    use crate::config::ManagerProgramConfig;
    use crate::test_support::ladder::market_with_side_ladder;
    use crate::test_support::signer_config::test_signer_config;

    const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(500)
        .create_async()
        .await;

    let market = market_with_side_ladder(RECEIVE_ADDRESS, "sell", 10, 2);
    let program = ManagerProgramConfig::default();
    let signer = test_signer_config(&server.url());

    let result =
        run_signer_denomination_phase(&program, &market, &signer, "xch", "xch", 1.0, "sell")
            .await
            .expect("phase");

    assert!(
        result.reason.starts_with("bootstrap_coin_list_failed:"),
        "expected coin list failure, got {}",
        result.reason
    );
    assert!(!result.ready);
}

#[tokio::test]
async fn run_signer_denomination_phase_rejects_nonzero_bootstrap_fee() {
    use crate::config::ManagerProgramConfig;
    use crate::test_support::ladder::market_with_side_ladder;
    use crate::test_support::signer_config::test_signer_config;

    const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
    const MOJO_PER_XCH: u64 = 1_000_000_000_000;
    let coin_body = format!(
        r#"{{
        "success": true,
        "coin_records": [{{
            "coin": {{
                "parent_coin_info": "c325057d788bee13367cb8e2d71ff3e209b5e94b31b296322ba1a143053fef5b",
                "puzzle_hash": "11cd056d9ec93f4612919b445e1ad9afeb7ef7739708c2d16cec4fd2d3cd5e63",
                "amount": {}
            }},
            "coinbase": false,
            "confirmed_block_index": 1,
            "spent": false,
            "spent_block_index": 0,
            "timestamp": 1
        }}]
    }}"#,
        MOJO_PER_XCH * 1000
    );
    let mut server = mockito::Server::new_async().await;
    let _coin_mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(coin_body)
        .create_async()
        .await;
    let _fee_mock = server
        .mock("POST", "/get_fee_estimate")
        .with_status(200)
        .with_body(r#"{"success":true,"estimates":[100,500]}"#)
        .create_async()
        .await;

    let market = market_with_side_ladder(RECEIVE_ADDRESS, "sell", 10, 2);
    let program = ManagerProgramConfig::default();
    let signer = test_signer_config(&server.url());

    let result =
        run_signer_denomination_phase(&program, &market, &signer, "xch", "xch", 1.0, "sell")
            .await
            .expect("phase");

    assert_eq!(result.reason, "signer_mixed_split_fee_not_supported");
    assert!(!result.ready);
    assert_eq!(result.fee_mojos, 500);
    assert!(!result.ready);
}

#[tokio::test]
async fn run_signer_denomination_phase_skips_when_ladder_already_ready() {
    use crate::config::ManagerProgramConfig;
    use crate::test_support::ladder::market_with_side_ladder;
    use crate::test_support::signer_config::test_signer_config;

    const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
    const MOJO_PER_XCH: u64 = 1_000_000_000_000;
    let coin_body = format!(
        r#"{{
        "success": true,
        "coin_records": [
            {{
                "coin": {{
                    "parent_coin_info": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "puzzle_hash": "11cd056d9ec93f4612919b445e1ad9afeb7ef7739708c2d16cec4fd2d3cd5e63",
                    "amount": {}
                }},
                "coinbase": false,
                "confirmed_block_index": 1,
                "spent": false,
                "spent_block_index": 0,
                "timestamp": 1
            }},
            {{
                "coin": {{
                    "parent_coin_info": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "puzzle_hash": "11cd056d9ec93f4612919b445e1ad9afeb7ef7739708c2d16cec4fd2d3cd5e63",
                    "amount": {}
                }},
                "coinbase": false,
                "confirmed_block_index": 1,
                "spent": false,
                "spent_block_index": 0,
                "timestamp": 1
            }},
            {{
                "coin": {{
                    "parent_coin_info": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                    "puzzle_hash": "11cd056d9ec93f4612919b445e1ad9afeb7ef7739708c2d16cec4fd2d3cd5e63",
                    "amount": {}
                }},
                "coinbase": false,
                "confirmed_block_index": 1,
                "spent": false,
                "spent_block_index": 0,
                "timestamp": 1
            }}
        ]
    }}"#,
        MOJO_PER_XCH * 10,
        MOJO_PER_XCH * 10,
        MOJO_PER_XCH * 10
    );
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(coin_body)
        .create_async()
        .await;

    let market = market_with_side_ladder(RECEIVE_ADDRESS, "sell", 10, 2);
    let program = ManagerProgramConfig::default();
    let signer = test_signer_config(&server.url());

    let result =
        run_signer_denomination_phase(&program, &market, &signer, "xch", "xch", 1.0, "sell")
            .await
            .expect("phase");

    assert_eq!(result.reason, "already_ready");
    assert!(!result.ready);
}

#[tokio::test]
async fn prepare_bootstrap_split_plan_returns_zero_fee_split_context() {
    use super::prepare_bootstrap_execution_plan;
    use crate::config::ManagerProgramConfig;
    use crate::test_support::ladder::market_with_side_ladder;
    use crate::test_support::signer_config::test_signer_config;

    const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
    const MOJO_PER_XCH: u64 = 1_000_000_000_000;
    let coin_body = format!(
        r#"{{
        "success": true,
        "coin_records": [{{
            "coin": {{
                "parent_coin_info": "c325057d788bee13367cb8e2d71ff3e209b5e94b31b296322ba1a143053fef5b",
                "puzzle_hash": "11cd056d9ec93f4612919b445e1ad9afeb7ef7739708c2d16cec4fd2d3cd5e63",
                "amount": {}
            }},
            "coinbase": false,
            "confirmed_block_index": 1,
            "spent": false,
            "spent_block_index": 0,
            "timestamp": 1
        }}]
    }}"#,
        MOJO_PER_XCH * 1000
    );
    let mut server = mockito::Server::new_async().await;
    let _coin_mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(coin_body)
        .create_async()
        .await;
    let _fee_mock = server
        .mock("POST", "/get_fee_estimate")
        .with_status(200)
        .with_body(r#"{"success":false}"#)
        .create_async()
        .await;

    let market = market_with_side_ladder(RECEIVE_ADDRESS, "sell", 10, 2);
    let program = ManagerProgramConfig {
        coin_ops_minimum_fee_mojos: 0,
        ..Default::default()
    };
    let signer = test_signer_config(&server.url());

    let plan_ctx =
        prepare_bootstrap_execution_plan(&program, &signer, &market, "sell", "xch", "xch", 1.0)
            .await
            .expect("phase result")
            .expect("execution plan");

    assert!(!plan_ctx.bootstrap_plan.requires_combine_first());
    assert_eq!(plan_ctx.fee_mojos, 0);
    assert_eq!(plan_ctx.fee_source, "config_minimum_fee_fallback");
    assert!(!plan_ctx.bootstrap_plan.output_amounts_base_units.is_empty());
}
