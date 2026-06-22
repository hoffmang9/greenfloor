use serde_json::json;

use super::{
    bootstrap_ladder_entries_for_side, resolve_bootstrap_split_fee, wallet_coin_spendable,
};
use crate::coinset::WalletUnspentCoin;
use crate::config::LadderEntry;
use crate::test_support::signer_config::test_signer_config;

#[test]
fn bootstrap_ladder_entries_for_sell_side_preserves_sizes() {
    let ladder = vec![LadderEntry {
        size_base_units: 25,
        target_count: 3,
        split_buffer_count: 1,
        combine_when_excess_factor: 2.0,
    }];
    let entries = bootstrap_ladder_entries_for_side("sell", &ladder, &json!({}), 1.0, "xch")
        .expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].size_base_units, 25);
    assert_eq!(entries[0].target_count, 3);
}

#[test]
fn bootstrap_ladder_entries_for_buy_side_converts_quote_sizes() {
    let ladder = vec![LadderEntry {
        size_base_units: 10,
        target_count: 2,
        split_buffer_count: 0,
        combine_when_excess_factor: 2.0,
    }];
    let pricing = json!({"quote_unit_mojo_multiplier": 1000});
    let entries = bootstrap_ladder_entries_for_side(
        "buy",
        &ladder,
        &pricing,
        2.0,
        "0000000000000000000000000000000000000000000000000000000000000001",
    )
    .expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].size_base_units, 20_000);
}

#[test]
fn wallet_coin_spendable_requires_confirmed_state() {
    let confirmed = WalletUnspentCoin {
        id: "a".repeat(64),
        name: "a".repeat(64),
        amount: 1,
        state: "CONFIRMED".to_string(),
    };
    let pending = WalletUnspentCoin {
        id: "b".repeat(64),
        name: "b".repeat(64),
        amount: 1,
        state: "PENDING".to_string(),
    };
    assert!(wallet_coin_spendable(&confirmed));
    assert!(!wallet_coin_spendable(&pending));
}

#[tokio::test]
async fn resolve_bootstrap_split_fee_uses_coinset_conservative_fee() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_fee_estimate")
        .with_status(200)
        .with_body(r#"{"success":true,"estimates":[100,500]}"#)
        .create_async()
        .await;

    let signer = test_signer_config(&server.url());

    let (fee_mojos, fee_source, lookup_error) =
        resolve_bootstrap_split_fee("mainnet", &signer, 99, 2).await;
    assert_eq!(fee_mojos, 500);
    assert_eq!(fee_source, "coinset_conservative_fee");
    assert!(lookup_error.is_none());
}

#[tokio::test]
async fn resolve_bootstrap_split_fee_falls_back_on_lookup_failure() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_fee_estimate")
        .with_status(500)
        .create_async()
        .await;

    let signer = test_signer_config(&server.url());

    let (fee_mojos, fee_source, lookup_error) =
        resolve_bootstrap_split_fee("mainnet", &signer, 99, 2).await;
    assert_eq!(fee_mojos, 99);
    assert_eq!(fee_source, "config_minimum_fee_fallback");
    assert!(lookup_error.is_some());
}

#[tokio::test]
async fn resolve_bootstrap_split_fee_falls_back_when_estimate_empty() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_fee_estimate")
        .with_status(200)
        .with_body(r#"{"success":false}"#)
        .create_async()
        .await;

    let signer = test_signer_config(&server.url());

    let (fee_mojos, fee_source, lookup_error) =
        resolve_bootstrap_split_fee("mainnet", &signer, 99, 2).await;
    assert_eq!(fee_mojos, 99);
    assert_eq!(fee_source, "config_minimum_fee_fallback");
    assert!(lookup_error.is_none());
}
