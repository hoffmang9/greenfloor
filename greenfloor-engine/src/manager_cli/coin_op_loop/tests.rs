use crate::coin_ops::{coin_op_should_stop, evaluate_coin_split_gate, SpendableCoin};

use super::combine::{run_coin_combine, CoinCombineBehavior, CoinCombineRequest};
use super::context::{enforce_split_lockup_guardrail, spendable_coins_for_gate};
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
    let code = super::list::run_coins_list(&harness.ctx, None, None, None)
        .await
        .expect("coins-list");
    assert_eq!(code, 2);
    let payload = pop_json(&harness.captured);
    assert_eq!(
        payload.get("error"),
        Some(&serde_json::json!("coin_list_requires_signer_backend"))
    );
}
