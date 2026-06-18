use std::path::PathBuf;

use crate::coin_ops::{coin_op_should_stop, evaluate_coin_split_gate, SpendableCoin};

use super::combine::run_coin_combine;
use super::context::{enforce_split_lockup_guardrail, spendable_coins_for_gate};
use super::split::run_coin_split;
use crate::manager_cli::context::ManagerContext;

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
    let code = enforce_split_lockup_guardrail(
        &spendable,
        &["coin-a".to_string(), "coin-b".to_string()],
        false,
        "asset-1",
    )
    .expect("guardrail payload")
    .map(|(code, _payload)| code);
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
    let exit = enforce_split_lockup_guardrail(
        &spendable,
        &["coin-a".to_string()],
        false,
        "asset-1",
    )
    .expect("guardrail");
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
    )
    .expect("guardrail");
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
    let gate = evaluate_coin_split_gate(
        &spendable_coins_for_gate(&spendable),
        "asset",
        100,
        2,
    );
    assert!(gate.ready);
    let (stop, reason) = coin_op_should_stop(true, Some(gate.ready), false, 1, 3);
    assert!(stop);
    assert_eq!(reason, "ready");
}

#[tokio::test]
async fn until_ready_requires_size_base_units() {
    let mgr = ManagerContext::for_test(
        PathBuf::from("/tmp/unused-program.yaml"),
        PathBuf::from("/tmp/unused-markets.yaml"),
    );
    let err = run_coin_split(
        &mgr,
        "mainnet",
        None,
        None,
        &[],
        10,
        2,
        false,
        None,
        true,
        3,
        false,
        false,
    )
    .await
    .expect_err("missing size");
    assert!(err.to_string().contains("until-ready mode requires --size-base-units"));
}

#[tokio::test]
async fn until_ready_disallows_no_wait() {
    let mgr = ManagerContext::for_test(
        PathBuf::from("/tmp/unused-program.yaml"),
        PathBuf::from("/tmp/unused-markets.yaml"),
    );
    let err = run_coin_split(
        &mgr,
        "mainnet",
        None,
        None,
        &[],
        10,
        2,
        true,
        Some(10),
        true,
        3,
        false,
        false,
    )
    .await
    .expect_err("no-wait conflict");
    assert!(err
        .to_string()
        .contains("until-ready mode requires wait mode"));
}

#[tokio::test]
async fn combine_until_ready_disallows_no_wait() {
    let mgr = ManagerContext::for_test(
        PathBuf::from("/tmp/unused-program.yaml"),
        PathBuf::from("/tmp/unused-markets.yaml"),
    );
    let err = run_coin_combine(
        &mgr,
        "mainnet",
        None,
        None,
        &[],
        2,
        None,
        true,
        Some(10),
        true,
        3,
    )
    .await
    .expect_err("no-wait conflict");
    assert!(err
        .to_string()
        .contains("until-ready mode requires wait mode"));
}
