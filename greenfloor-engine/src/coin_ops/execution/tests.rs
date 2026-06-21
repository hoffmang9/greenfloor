use std::collections::HashSet;

use crate::coin_ops::SpendableCoin;
use crate::config::{load_program_bundle, ManagerProgramConfig, MarketConfig};
use crate::test_support::minimal_program::{
    write_minimal_program_with_signer, MinimalProgramParams,
};

use super::{CoinOpExecContext, CoinOpTestOverrides};

fn sample_exec_context(overrides: CoinOpTestOverrides) -> CoinOpExecContext {
    let dir = tempfile::tempdir().expect("tempdir");
    let program_path = dir.path().join("program.yaml");
    write_minimal_program_with_signer(
        &program_path,
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    let bundle = load_program_bundle(&program_path).expect("program bundle");
    CoinOpExecContext {
        signer_config: bundle.signer,
        market: MarketConfig {
            market_id: "m1".to_string(),
            enabled: true,
            base_asset: "xch".to_string(),
            base_symbol: "XCH".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "stable".to_string(),
            receive_address: "xch1test".to_string(),
            signer_key_id: "key-1".to_string(),
            mode: "sell_only".to_string(),
            pricing: serde_json::json!({}),
            cancel_move_threshold_bps: None,
            ladders: std::collections::HashMap::new(),
        },
        program: ManagerProgramConfig {
            network: "mainnet".to_string(),
            ..Default::default()
        },
        resolved_base_asset_id: "xch".to_string(),
        base_unit_mojo_multiplier: 1,
        combine_input_cap: 100,
        watched_coin_ids: HashSet::new(),
        test_overrides: overrides,
    }
}

#[tokio::test]
async fn list_spendable_coins_uses_wallet_override() {
    let ctx = sample_exec_context(CoinOpTestOverrides {
        wallet_coins: Some(vec![SpendableCoin {
            id: "coin-a".to_string(),
            amount: 500,
        }]),
        ..CoinOpTestOverrides::default()
    });
    let coins = ctx.list_spendable_coins().await.expect("wallet coins");
    assert_eq!(coins.len(), 1);
    assert_eq!(coins[0].id, "coin-a");
}

#[tokio::test]
async fn execute_mixed_split_uses_operation_id_override() {
    let ctx = sample_exec_context(CoinOpTestOverrides {
        mixed_split_operation_id: Some("op-test-123".to_string()),
        ..CoinOpTestOverrides::default()
    });
    let operation_id = ctx
        .execute_mixed_split(vec![1000, 1000], &["coin-a".to_string()], 0)
        .await
        .expect("mixed split");
    assert_eq!(operation_id, "op-test-123");
}
