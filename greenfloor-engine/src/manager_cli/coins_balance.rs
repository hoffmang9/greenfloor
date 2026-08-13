//! `coins-balance`: vault-controlled CAT total (receive + known unreturned makers).

use serde_json::json;

use crate::coin_ops::vault_controlled_balance;
use crate::coinset::{
    client_for_signer_on_network, list_wallet_unspent_coins_for_signer, LiveCoinset,
};
use crate::config::{
    load_gated_operator_market, GatedOperatorMarketLoadRequest, OperatorMarketCommand,
};
use crate::error::{SignerError, SignerResult};
use crate::manager_cli::asset_resolve::resolve_market_inventory_asset_id;
use crate::manager_cli::context::ManagerContext;
use crate::storage::{resolve_state_db_path, SqliteStore};

/// Vault-controlled balance for one asset: receive inventory + known unreturned makers.
///
/// # Errors
///
/// Returns an error when market load, Coinset, or `SQLite` queries fail.
pub async fn run_coins_balance(
    mgr: &ManagerContext,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    asset: Option<&str>,
) -> SignerResult<i32> {
    let loaded = load_gated_operator_market(&GatedOperatorMarketLoadRequest {
        program_path: &mgr.program_config,
        markets_path: &mgr.markets_config,
        testnet_markets_path: mgr.testnet_markets_path(),
        cats_path: Some(&mgr.cats_config),
        network,
        market_id,
        pair,
        command: OperatorMarketCommand::CoinList,
    })?;
    let market = &loaded.market_row;
    let receive_address = market.receive_address.trim();
    if receive_address.is_empty() {
        return Err(SignerError::Other(
            "market missing receive_address for coins-balance".to_string(),
        ));
    }
    let resolver = loaded.asset_resolver();
    let list_asset_id =
        resolve_market_inventory_asset_id(&resolver, &market.base_asset, asset).await?;

    let receive_coins = list_wallet_unspent_coins_for_signer(
        &loaded.operator_network,
        &loaded.signer,
        receive_address,
        &list_asset_id,
    )
    .await?;
    let receive_amount: u64 = receive_coins.iter().map(|coin| coin.amount).sum();

    let db_path = resolve_state_db_path(&loaded.program.home_dir, mgr.state_db_override());
    let store = SqliteStore::open(&db_path)?;
    let coinset = client_for_signer_on_network(&loaded.signer, &loaded.operator_network)?;
    let backend = LiveCoinset(&coinset);
    let balance = vault_controlled_balance(
        &store,
        &backend,
        &market.market_id,
        &list_asset_id,
        receive_amount,
    )
    .await?;

    let unreturned_coins: Vec<_> = balance
        .unreturned_coins
        .iter()
        .map(|coin| {
            json!({
                "coin_id": coin.coin_id,
                "amount": coin.amount,
                "fixed_delegated_puzzle_hash": coin.fixed_delegated_puzzle_hash,
                "offer_id": coin.offer_id,
                "state": coin.state,
                "size_base_units": coin.size_base_units,
                "reclaimable": coin.reclaimable,
            })
        })
        .collect();

    let payload = json!({
        "op": "coins-balance",
        "network": loaded.operator_network,
        "market_id": market.market_id,
        "asset": list_asset_id,
        "receive_address": receive_address,
        "receive_amount": balance.receive_amount,
        "receive_units": crate::coin_ops::cat_units_display_from_mojos(balance.receive_amount),
        "unreturned_amount": balance.unreturned_amount,
        "unreturned_units": crate::coin_ops::cat_units_display_from_mojos(balance.unreturned_amount),
        "vault_controlled_amount": balance.vault_controlled_amount,
        "vault_controlled_units": crate::coin_ops::cat_units_display_from_mojos(balance.vault_controlled_amount),
        "unreturned_coins": unreturned_coins,
        "note": "Open makers are listed with reclaimable=false; reclaim idle/expired via offers-reclaim-presplit.",
    });
    mgr.emit_json(&payload)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use crate::cycle::{unreturned_row_priority, OfferLifecycleState, ReconcileState};

    fn priority_for(raw: &str) -> u8 {
        unreturned_row_priority(ReconcileState::parse(raw).ok().as_ref())
    }

    #[test]
    fn cat_units_preserve_fractional_mojos() {
        assert_eq!(
            crate::coin_ops::cat_units_display_from_mojos(239_950).to_string(),
            "239.95"
        );
        assert_eq!(
            crate::coin_ops::cat_units_display_from_mojos(10_500).to_string(),
            "10.5"
        );
        assert_eq!(
            crate::coin_ops::cat_units_display_from_mojos(1_000).to_string(),
            "1"
        );
        assert_eq!(
            crate::coin_ops::cat_units_display_from_mojos(100).to_string(),
            "0.1"
        );
        assert_eq!(
            crate::coin_ops::cat_units_display_from_mojos(0).to_string(),
            "0"
        );
    }

    #[test]
    fn unreturned_priority_orders_open_before_expired() {
        assert!(priority_for("open") < priority_for("expired"));
        assert!(priority_for("expired") < priority_for("cancelled"));
        assert_eq!(
            unreturned_row_priority(Some(&ReconcileState::Lifecycle(
                OfferLifecycleState::RefreshDue
            ))),
            0
        );
        assert_eq!(
            unreturned_row_priority(Some(&ReconcileState::PendingVisibility)),
            0
        );
        assert_eq!(
            unreturned_row_priority(Some(&ReconcileState::Lifecycle(
                OfferLifecycleState::MempoolObserved
            ))),
            0
        );
        assert_eq!(unreturned_row_priority(None), 2);
    }

    #[tokio::test]
    async fn run_coins_balance_empty_receive_and_no_unreturned() {
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
        let db_path = dir.path().join("state.db");
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
            .state_db(db_path.to_string_lossy().into_owned())
            .build_capturing();
        let code = crate::manager_cli::commands::ManagerCommands::CoinsBalance {
            market_id: Some("m1".to_string()),
            pair: None,
            network: "mainnet".to_string(),
            asset: String::new(),
        }
        .run(&harness.ctx)
        .await
        .expect("coins-balance command");
        assert_eq!(code, 0);
        let payload = pop_json(&harness.captured);
        assert_eq!(payload.get("receive_amount"), Some(&serde_json::json!(0)));
        assert_eq!(
            payload.get("unreturned_amount"),
            Some(&serde_json::json!(0))
        );
        assert_eq!(
            payload.get("vault_controlled_amount"),
            Some(&serde_json::json!(0))
        );
    }
}
