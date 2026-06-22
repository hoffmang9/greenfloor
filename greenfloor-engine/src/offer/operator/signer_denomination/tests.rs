use crate::coinset::WalletUnspentCoin;
use crate::offer::bootstrap::{BootstrapCoin, BootstrapPlan, PlannerLadderRow};

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
        source_coin_id: "coin-a".to_string(),
        source_amount: 50_000,
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
    let refreshed = vec![BootstrapCoin {
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
