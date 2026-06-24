use crate::coin_ops::execution::CoinOpTestOverrides;
use crate::coin_ops::{coin_op_should_stop, evaluate_coin_split_gate, SpendableCoin};

use super::combine::{run_coin_combine, CoinCombineBehavior, CoinCombineRequest};
use super::context::{enforce_split_lockup_guardrail, spendable_coins_for_gate};
use super::split::run_coin_split_with_test_overrides;
use super::split::{run_coin_split, CoinSplitBehavior, CoinSplitGating, CoinSplitRequest};
use super::until_ready::UntilReadyWaitMode;
use crate::manager_cli::test_support::ManagerContextBuilder;

#[test]
fn lockup_guardrail_blocks_when_all_spendable_selected() {
    let spendable = vec![
        SpendableCoin {
            id: "coin-a".to_string(),
            amount: 100,
        },
        SpendableCoin {
            id: "coin-b".to_string(),
            amount: 200,
        },
    ];
    let guardrail = enforce_split_lockup_guardrail(
        &spendable,
        &["coin-a".to_string(), "coin-b".to_string()],
        false,
        "asset-1",
    );
    let code = guardrail.map(|(code, _payload)| code);
    assert_eq!(code, Some(2));
}

#[test]
fn lockup_guardrail_allows_partial_selection() {
    let spendable = vec![
        SpendableCoin {
            id: "coin-a".to_string(),
            amount: 100,
        },
        SpendableCoin {
            id: "coin-b".to_string(),
            amount: 200,
        },
    ];
    let exit =
        enforce_split_lockup_guardrail(&spendable, &["coin-a".to_string()], false, "asset-1");
    assert!(exit.is_none());
}

#[test]
fn lockup_guardrail_allows_override_when_flag_set() {
    let spendable = vec![
        SpendableCoin {
            id: "coin-a".to_string(),
            amount: 100,
        },
        SpendableCoin {
            id: "coin-b".to_string(),
            amount: 200,
        },
    ];
    let exit = enforce_split_lockup_guardrail(
        &spendable,
        &["coin-a".to_string(), "coin-b".to_string()],
        true,
        "asset-1",
    );
    assert!(exit.is_none());
}

#[test]
fn split_gate_ready_skips_execution_path() {
    let spendable = vec![
        SpendableCoin {
            id: "a".to_string(),
            amount: 100,
        },
        SpendableCoin {
            id: "b".to_string(),
            amount: 100,
        },
        SpendableCoin {
            id: "c".to_string(),
            amount: 200,
        },
    ];
    let gate = evaluate_coin_split_gate(&spendable_coins_for_gate(&spendable), "asset", 100, 2);
    assert!(gate.ready);
    let (stop, reason) = coin_op_should_stop(true, Some(gate.ready), false, 1, 3);
    assert!(stop);
    assert_eq!(reason, "ready");
}

#[tokio::test]
async fn until_ready_requires_size_base_units() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mgr = ManagerContextBuilder::new(
        dir.path().join("unused-program.yaml"),
        dir.path().join("unused-markets.yaml"),
    )
    .scratch_dir(dir.path().to_path_buf())
    .json_compact(false)
    .build();
    let err = run_coin_split(CoinSplitRequest {
        mgr: &mgr,
        network: "mainnet",
        market_id: None,
        pair: None,
        coin_ids: &[],
        amount_per_coin: 10,
        number_of_coins: 2,
        behavior: CoinSplitBehavior {
            wait: UntilReadyWaitMode {
                until_ready: true,
                no_wait: false,
            },
            gating: CoinSplitGating {
                allow_lock_all_spendable: false,
                force_split_when_ready: false,
            },
        },
        size_base_units: None,
        max_iterations: 3,
    })
    .await
    .expect_err("missing size");
    assert!(err
        .to_string()
        .contains("until-ready mode requires --size-base-units"));
}

#[tokio::test]
async fn until_ready_disallows_no_wait() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mgr = ManagerContextBuilder::new(
        dir.path().join("unused-program.yaml"),
        dir.path().join("unused-markets.yaml"),
    )
    .scratch_dir(dir.path().to_path_buf())
    .json_compact(false)
    .build();
    let err = run_coin_split(CoinSplitRequest {
        mgr: &mgr,
        network: "mainnet",
        market_id: None,
        pair: None,
        coin_ids: &[],
        amount_per_coin: 10,
        number_of_coins: 2,
        behavior: CoinSplitBehavior {
            wait: UntilReadyWaitMode {
                until_ready: true,
                no_wait: true,
            },
            gating: CoinSplitGating {
                allow_lock_all_spendable: false,
                force_split_when_ready: false,
            },
        },
        size_base_units: Some(10),
        max_iterations: 3,
    })
    .await
    .expect_err("no-wait conflict");
    assert!(err
        .to_string()
        .contains("until-ready mode requires wait mode"));
}

#[tokio::test]
async fn combine_until_ready_requires_size_base_units() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mgr = ManagerContextBuilder::new(
        dir.path().join("unused-program.yaml"),
        dir.path().join("unused-markets.yaml"),
    )
    .scratch_dir(dir.path().to_path_buf())
    .json_compact(false)
    .build();
    let err = run_coin_combine(CoinCombineRequest {
        mgr: &mgr,
        network: "mainnet",
        market_id: None,
        pair: None,
        coin_ids: &[],
        number_of_coins: 2,
        asset_id: None,
        behavior: CoinCombineBehavior::from_cli(true, false),
        size_base_units: None,
        max_iterations: 3,
    })
    .await
    .expect_err("missing size");
    assert!(err
        .to_string()
        .contains("until-ready mode requires --size-base-units"));
}

#[tokio::test]
async fn combine_until_ready_disallows_no_wait() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mgr = ManagerContextBuilder::new(
        dir.path().join("unused-program.yaml"),
        dir.path().join("unused-markets.yaml"),
    )
    .scratch_dir(dir.path().to_path_buf())
    .json_compact(false)
    .build();
    let err = run_coin_combine(CoinCombineRequest {
        mgr: &mgr,
        network: "mainnet",
        market_id: None,
        pair: None,
        coin_ids: &[],
        number_of_coins: 2,
        asset_id: None,
        behavior: CoinCombineBehavior::from_cli(true, true),
        size_base_units: Some(10),
        max_iterations: 3,
    })
    .await
    .expect_err("no-wait conflict");
    assert!(err
        .to_string()
        .contains("until-ready mode requires wait mode"));
}

#[tokio::test]
async fn coins_list_requires_signer_backend() {
    use crate::manager_cli::test_support::{pop_json, ManagerContextBuilder};
    use crate::minimal_program_template::{write_minimal_program, MinimalProgramParams};

    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_minimal_program(
        &program,
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    std::fs::write(
        &markets,
        r#"markets:
  - id: m1
    enabled: true
    base_asset: "asset1"
    base_symbol: "AS1"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 10
      bucket_counts:
        1: 0
    ladders:
      sell:
        - size_base_units: 1
          target_count: 1
          split_buffer_count: 0
          combine_when_excess_factor: 2.0
"#,
    )
    .expect("write markets");
    let harness = ManagerContextBuilder::new(program, markets)
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
    let code = super::list::run_coins_list(&harness.ctx, "mainnet", None, None, None, None, None)
        .await
        .expect("coins-list");
    assert_eq!(code, 2);
    let payload = pop_json(&harness.captured);
    assert_eq!(
        payload.get("error"),
        Some(&serde_json::json!("coin_list_requires_signer_backend"))
    );
}

#[tokio::test]
async fn coins_list_returns_empty_wallet_with_signer_backend() {
    use crate::manager_cli::test_support::{pop_json, ManagerContextBuilder};
    use crate::minimal_program_template::{
        write_minimal_program_with_signer_coinset, MinimalProgramParams,
    };

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[]}"#)
        .create_async()
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_minimal_program_with_signer_coinset(
        &program,
        &server.url(),
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    std::fs::write(
        &markets,
        r#"markets:
  - id: m1
    enabled: true
    base_asset: "xch"
    base_symbol: "XCH"
    quote_asset: "xch"
    quote_asset_type: "stable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 10
      bucket_counts:
        1: 0
    ladders:
      sell:
        - size_base_units: 1
          target_count: 1
          split_buffer_count: 0
          combine_when_excess_factor: 2.0
"#,
    )
    .expect("write markets");
    let harness = ManagerContextBuilder::new(program, markets)
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
    let code = super::list::run_coins_list(&harness.ctx, "mainnet", None, None, None, None, None)
        .await
        .expect("coins-list");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("coin_count"), Some(&serde_json::json!(0)));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn coins_list_applies_testnet_markets_overlay_for_receive_address() {
    use crate::manager_cli::test_support::{pop_json, ManagerContextBuilder};
    use crate::minimal_program_template::{
        write_minimal_program_with_signer_coinset, MinimalProgramParams,
    };

    const MAINNET_RECEIVE: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
    const TESTNET_RECEIVE: &str = "txch1t37dk4kxmptw9eceyjvxn55cfrh827yf5f0nnnm2t6r882nkl66qknnt9k";

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[]}"#)
        .create_async()
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    let testnet_markets = dir.path().join("testnet-markets.yaml");
    write_minimal_program_with_signer_coinset(
        &program,
        &server.url(),
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    std::fs::write(
        &markets,
        format!(
            r#"markets:
  - id: z-mainnet
    enabled: true
    base_asset: "xch"
    base_symbol: "XCH"
    quote_asset: "xch"
    quote_asset_type: "stable"
    signer_key_id: "key-main-1"
    receive_address: "{MAINNET_RECEIVE}"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 10
      bucket_counts:
        1: 0
    ladders:
      sell:
        - size_base_units: 1
          target_count: 1
          split_buffer_count: 0
          combine_when_excess_factor: 2.0
"#
        ),
    )
    .expect("write markets");
    std::fs::write(
        &testnet_markets,
        format!(
            r#"markets:
  - id: a-testnet
    enabled: true
    base_asset: "xch"
    base_symbol: "XCH"
    quote_asset: "xch"
    quote_asset_type: "stable"
    signer_key_id: "key-main-1"
    receive_address: "{TESTNET_RECEIVE}"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 10
      bucket_counts:
        1: 0
    ladders:
      sell:
        - size_base_units: 1
          target_count: 1
          split_buffer_count: 0
          combine_when_excess_factor: 2.0
"#
        ),
    )
    .expect("write testnet markets");
    let harness = ManagerContextBuilder::new(program, markets)
        .testnet_markets(testnet_markets)
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
    let code = super::list::run_coins_list(
        &harness.ctx,
        "mainnet",
        Some("a-testnet"),
        None,
        None,
        None,
        None,
    )
    .await
    .expect("coins-list");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(
        payload.get("receive_address"),
        Some(&serde_json::json!(TESTNET_RECEIVE))
    );
    assert_eq!(
        payload.get("market_id"),
        Some(&serde_json::json!("a-testnet"))
    );
}

fn write_split_test_markets(path: &std::path::Path) {
    std::fs::write(
        path,
        r#"markets:
  - id: m1
    enabled: true
    base_asset: "xch"
    base_symbol: "XCH"
    quote_asset: "xch"
    quote_asset_type: "stable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 10
      bucket_counts:
        1: 0
    ladders:
      sell:
        - size_base_units: 100
          target_count: 2
          split_buffer_count: 0
          combine_when_excess_factor: 2.0
"#,
    )
    .expect("write markets");
}

#[tokio::test]
async fn coin_split_executes_with_test_overrides() {
    use crate::coin_ops::SpendableCoin;
    use crate::manager_cli::test_support::{pop_json, ManagerContextBuilder};
    use crate::minimal_program_template::{
        write_minimal_program_with_signer, MinimalProgramParams,
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_minimal_program_with_signer(
        &program,
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    write_split_test_markets(&markets);
    let coin_id = "a".repeat(64);
    let harness = ManagerContextBuilder::new(program, markets)
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
    let code = run_coin_split_with_test_overrides(
        CoinSplitRequest {
            mgr: &harness.ctx,
            network: "mainnet",
            market_id: Some("m1"),
            pair: None,
            coin_ids: std::slice::from_ref(&coin_id),
            amount_per_coin: 100,
            number_of_coins: 2,
            behavior: CoinSplitBehavior {
                wait: UntilReadyWaitMode {
                    until_ready: false,
                    no_wait: true,
                },
                gating: CoinSplitGating {
                    allow_lock_all_spendable: true,
                    force_split_when_ready: true,
                },
            },
            size_base_units: None,
            max_iterations: 1,
        },
        CoinOpTestOverrides {
            wallet_coins: Some(vec![SpendableCoin {
                id: coin_id.clone(),
                amount: 1_000_000,
            }]),
            mixed_split_operation_id: Some("split-op-test".to_string()),
        },
    )
    .await
    .expect("coin-split");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("op"), Some(&serde_json::json!("coin-split")));
    let operations = payload
        .get("operations")
        .and_then(|value| value.as_array())
        .expect("operations");
    assert!(operations.iter().any(|row| {
        row.get("signature_request_id") == Some(&serde_json::json!("split-op-test"))
    }));
}
