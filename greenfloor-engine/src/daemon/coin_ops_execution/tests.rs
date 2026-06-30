use std::collections::HashSet;

use crate::coin_ops::execution::{
    resolve_combine_input_cap, CoinOpExecContext, CoinOpTestOverrides,
};
use crate::coin_ops::{CoinOpKind, CoinOpPlan, CoinOpPlanReason, SpendableCoin};
use crate::config::{
    empty_cat_ticker_index, load_program_bundle, GatedOperatorMarket, ManagerProgramConfig,
    SignerConfig,
};
use crate::daemon::coin_ops_execution::{
    execute_managed_coin_op_plans, execute_managed_coin_op_plans_with_test_overrides,
    CoinOpExecutionResult,
};
use crate::test_support::market_config::sample_market;
use crate::test_support::minimal_program::{
    write_minimal_program_with_signer, MinimalProgramParams,
};

use super::combine::execute_daemon_combine_plan;
use super::split::execute_daemon_split_plan;

async fn run_daemon_split_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> (Vec<crate::daemon::coin_ops_execution::CoinOpExecItem>, u64) {
    Box::pin(execute_daemon_split_plan(ctx, plan)).await
}

fn sample_plan(op_type: CoinOpKind) -> CoinOpPlan {
    CoinOpPlan {
        op_type,
        size_base_units: 10,
        op_count: 2,
        reason: CoinOpPlanReason::ExcessOnlyPolicy,
    }
}

fn assert_skipped_all(result: &CoinOpExecutionResult, reason: &str) {
    assert_eq!(result.executed_count, 0);
    assert_eq!(result.status, "skipped");
    assert!(result.items.iter().all(|item| item.status == "skipped"));
    assert!(result.items.iter().all(|item| item.reason.contains(reason)));
}

fn test_coin_id(label: char) -> String {
    format!("{label:0>64}")
}

fn test_exec_context(
    market: crate::config::MarketConfig,
    spendable: Vec<SpendableCoin>,
    mixed_split_operation_id: Option<&str>,
) -> CoinOpExecContext {
    CoinOpExecContext {
        gated: GatedOperatorMarket::assemble(
            ManagerProgramConfig {
                coin_ops_split_fee_mojos: 0,
                coin_ops_combine_fee_mojos: 0,
                ..Default::default()
            },
            crate::test_support::signer_config::test_signer_config("https://example.test"),
            market,
            empty_cat_ticker_index(),
            "mainnet",
        ),
        resolved_base_asset_id: "xch".to_string(),
        base_unit_mojo_multiplier: 1_000,
        combine_input_cap: resolve_combine_input_cap(),
        watched_coin_ids: HashSet::new(),
        test_overrides: CoinOpTestOverrides {
            wallet_coins: Some(spendable),
            mixed_split_operation_id: mixed_split_operation_id.map(str::to_string),
        },
    }
}

fn sample_gated_market(
    program: ManagerProgramConfig,
    signer: SignerConfig,
    market_row: &crate::config::MarketConfig,
    ticker_index: crate::config::CatTickerIndex,
) -> GatedOperatorMarket {
    GatedOperatorMarket::assemble(program, signer, market_row.clone(), ticker_index, "mainnet")
}

fn minimal_program_bundle(dir: &tempfile::TempDir) -> crate::config::ProgramConfigBundle {
    let program_path = dir.path().join("program.yaml");
    write_minimal_program_with_signer(
        &program_path,
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    load_program_bundle(&program_path).expect("bundle")
}

#[tokio::test]
async fn execute_managed_coin_op_plans_skips_when_receive_address_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = minimal_program_bundle(&dir);
    let market = sample_market("");
    let plans = vec![
        sample_plan(CoinOpKind::Split),
        sample_plan(CoinOpKind::Combine),
    ];

    let empty_index = empty_cat_ticker_index();
    let result = execute_managed_coin_op_plans(
        sample_gated_market(bundle.program, bundle.signer, &market, empty_index),
        &plans,
        &HashSet::<String>::default(),
    )
    .await;

    assert_skipped_all(&result, "signer_coin_ops_missing_receive_address");
    assert_eq!(result.planned_count, 2);
}

#[tokio::test]
async fn execute_managed_coin_op_plans_dry_run_plans_without_execution() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program_path = dir.path().join("program.yaml");
    write_minimal_program_with_signer(
        &program_path,
        MinimalProgramParams {
            home_dir: dir.path(),
            dry_run: true,
            ..Default::default()
        },
    );
    let mut bundle = load_program_bundle(&program_path).expect("bundle");
    bundle.program.runtime_dry_run = true;
    let market = sample_market("xch1test");
    let plans = vec![sample_plan(CoinOpKind::Split)];

    let empty_index = empty_cat_ticker_index();
    let result = execute_managed_coin_op_plans(
        sample_gated_market(bundle.program, bundle.signer, &market, empty_index),
        &plans,
        &HashSet::<String>::default(),
    )
    .await;

    assert_eq!(result.executed_count, 0);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].status, "planned");
    assert_eq!(result.items[0].reason, "dry_run:signer");
}

#[tokio::test]
async fn execute_managed_coin_op_plans_skips_invalid_plans() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = minimal_program_bundle(&dir);
    let market = sample_market("xch1test");
    let plans = vec![CoinOpPlan {
        op_type: CoinOpKind::Split,
        size_base_units: 0,
        op_count: 0,
        reason: CoinOpPlanReason::ExcessOnlyPolicy,
    }];

    let empty_index = empty_cat_ticker_index();
    let result = execute_managed_coin_op_plans(
        sample_gated_market(bundle.program, bundle.signer, &market, empty_index),
        &plans,
        &HashSet::<String>::default(),
    )
    .await;

    assert_eq!(result.executed_count, 0);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].reason, "invalid_plan");
}

#[tokio::test]
async fn execute_managed_coin_op_plans_executes_split_and_combine_via_runner_overrides() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = minimal_program_bundle(&dir);
    let mut market = sample_market("xch1test");
    market.base_asset = test_coin_id('f');
    let plans = vec![
        sample_plan(CoinOpKind::Split),
        sample_plan(CoinOpKind::Combine),
    ];

    let empty_index = empty_cat_ticker_index();
    let result = execute_managed_coin_op_plans_with_test_overrides(
        sample_gated_market(bundle.program, bundle.signer, &market, empty_index),
        &plans,
        &HashSet::<String>::default(),
        CoinOpTestOverrides {
            wallet_coins: Some(vec![
                SpendableCoin {
                    id: test_coin_id('a'),
                    amount: 100_000,
                },
                SpendableCoin {
                    id: test_coin_id('b'),
                    amount: 10_000,
                },
                SpendableCoin {
                    id: test_coin_id('c'),
                    amount: 10_000,
                },
            ]),
            mixed_split_operation_id: Some("managed-op-test".to_string()),
        },
    )
    .await;

    assert_eq!(result.executed_count, 2);
    assert_eq!(result.items.len(), 2);
    assert!(result
        .items
        .iter()
        .any(|item| item.reason == "signer_split_submitted"));
    assert!(result
        .items
        .iter()
        .any(|item| item.reason == "signer_combine_submitted"));
    assert!(result
        .items
        .iter()
        .all(|item| item.operation_id.as_deref() == Some("managed-op-test")));
}

#[tokio::test]
async fn run_daemon_split_plan_defers_single_output_to_bootstrap() {
    let mut market = sample_market("xch1test");
    market.base_asset = "b".repeat(64);
    let ctx = test_exec_context(
        market,
        vec![
            SpendableCoin {
                id: test_coin_id('a'),
                amount: 65_000,
            },
            SpendableCoin {
                id: test_coin_id('b'),
                amount: 20_000,
            },
            SpendableCoin {
                id: test_coin_id('c'),
                amount: 11_000,
            },
            SpendableCoin {
                id: test_coin_id('d'),
                amount: 4_000,
            },
        ],
        None,
    );
    let plan = CoinOpPlan {
        op_type: CoinOpKind::Split,
        size_base_units: 100,
        op_count: 1,
        reason: CoinOpPlanReason::LowWatermarkBufferDeficit,
    };

    let (items, executed) = run_daemon_split_plan(&ctx, &plan).await;

    assert_eq!(executed, 0);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].reason, "bootstrap_primary_shape_deferred");
}

#[tokio::test]
async fn run_daemon_split_plan_uses_ninety_bu_remnant_for_john_deere_buffer_deficit() {
    use crate::test_support::eco181_bootstrap_inventory::john_deere_after_combine_inventory_rows;
    use crate::test_support::ladder::market_with_eco181_sell_ladder;

    const MULTIPLIER: i64 = 1000;
    let mut market = market_with_eco181_sell_ladder("xch1test");
    market.base_asset = "b".repeat(64);
    let spendable: Vec<SpendableCoin> = john_deere_after_combine_inventory_rows()
        .into_iter()
        .map(|(id, amount)| SpendableCoin {
            id,
            amount: amount.saturating_mul(MULTIPLIER),
        })
        .collect();
    let ctx = test_exec_context(market, spendable, Some("split-op-test"));
    let plan = CoinOpPlan {
        op_type: CoinOpKind::Split,
        size_base_units: 10,
        op_count: 1,
        reason: CoinOpPlanReason::LowWatermarkBufferDeficit,
    };

    let (items, executed) = run_daemon_split_plan(&ctx, &plan).await;

    assert_eq!(executed, 1);
    assert_eq!(items[0].reason, "signer_split_submitted");
}

#[tokio::test]
async fn run_daemon_low_watermark_split_skips_protection_when_sell_ladder_empty() {
    use crate::test_support::ladder::empty_ladders_market;

    let market = empty_ladders_market("xch1test");
    let spendable = vec![SpendableCoin {
        id: test_coin_id('a'),
        amount: 100_000,
    }];
    let ctx = test_exec_context(market, spendable, Some("split-op-test"));
    let plan = CoinOpPlan {
        op_type: CoinOpKind::Split,
        size_base_units: 10,
        op_count: 1,
        reason: CoinOpPlanReason::LowWatermarkBufferDeficit,
    };

    let (items, executed) = run_daemon_split_plan(&ctx, &plan).await;

    assert_eq!(executed, 1);
    assert_eq!(items[0].reason, "signer_split_submitted");
}

#[tokio::test]
async fn run_daemon_split_plan_submits_single_output_when_source_is_larger() {
    let mut market = sample_market("xch1test");
    market.base_asset = "b".repeat(64);
    let ctx = test_exec_context(
        market,
        vec![SpendableCoin {
            id: test_coin_id('a'),
            amount: 150_000,
        }],
        Some("split-op"),
    );
    let plan = CoinOpPlan {
        op_type: CoinOpKind::Split,
        size_base_units: 100,
        op_count: 1,
        reason: CoinOpPlanReason::LowWatermarkBufferDeficit,
    };

    let (items, executed) = run_daemon_split_plan(&ctx, &plan).await;

    assert_eq!(executed, 1);
    assert_eq!(items[0].reason, "signer_split_submitted");
}

#[tokio::test]
async fn run_daemon_split_plan_skips_when_no_spendable_coins() {
    let ctx = test_exec_context(sample_market("xch1test"), Vec::new(), None);
    let plan = sample_plan(CoinOpKind::Split);

    let (items, executed) = run_daemon_split_plan(&ctx, &plan).await;

    assert_eq!(executed, 0);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].reason, "no_spendable_split_coin_available");
}

#[tokio::test]
async fn run_daemon_split_plan_skips_when_amount_below_minimum() {
    let mut market = sample_market("xch1test");
    market.base_asset = "b".repeat(64);
    let ctx = test_exec_context(
        market,
        vec![SpendableCoin {
            id: test_coin_id('h'),
            amount: 100_000,
        }],
        None,
    );
    let plan = CoinOpPlan {
        op_type: CoinOpKind::Split,
        size_base_units: 0,
        op_count: 2,
        reason: CoinOpPlanReason::ExcessOnlyPolicy,
    };

    let (items, executed) = run_daemon_split_plan(&ctx, &plan).await;

    assert_eq!(executed, 0);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].reason, "split_amount_below_coin_op_minimum");
}

#[tokio::test]
async fn run_daemon_split_plan_submits_when_spendable_coin_available() {
    let mut market = sample_market("xch1test");
    market.base_asset = test_coin_id('f');
    let ctx = test_exec_context(
        market,
        vec![SpendableCoin {
            id: test_coin_id('a'),
            amount: 100_000,
        }],
        Some("split-op-test"),
    );
    let plan = sample_plan(CoinOpKind::Split);

    let (items, executed) = run_daemon_split_plan(&ctx, &plan).await;

    assert_eq!(executed, 1);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].status, "executed");
    assert_eq!(items[0].reason, "signer_split_submitted");
    assert_eq!(items[0].operation_id.as_deref(), Some("split-op-test"));
}

#[tokio::test]
async fn execute_daemon_combine_plan_skips_when_insufficient_inputs() {
    let ctx = test_exec_context(
        sample_market("xch1test"),
        vec![SpendableCoin {
            id: test_coin_id('d'),
            amount: 10_000,
        }],
        None,
    );
    let plan = sample_plan(CoinOpKind::Combine);

    let (items, executed) = Box::pin(execute_daemon_combine_plan(&ctx, &plan)).await;

    assert_eq!(executed, 0);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].reason, "no_spendable_combine_coin_available");
}
