use std::collections::HashSet;

use crate::coinset::coin_id_from_record;
use crate::vault_coinset_scan::cat_scan_fixtures::{
    build_multi_dust_cat_scan_fixtures, mount_cat_scan_mocks,
};
use crate::vault_coinset_scan::request::ScanRequest;
use crate::vault_coinset_scan::types::{AssetTypeFilter, CoinKind};

use super::ScanState;

#[tokio::test]
async fn run_vault_coinset_scan_discovers_cat_via_parent_spend() {
    let fixtures = build_multi_dust_cat_scan_fixtures(&[5_000]);
    let coin = &fixtures.coins[0];

    let mut server = mockito::Server::new_async().await;
    mount_cat_scan_mocks(&mut server, &fixtures).await;

    let dir = tempfile::tempdir().expect("tempdir");
    let request = ScanRequest {
        network: "mainnet".to_string(),
        coinset_base_url: Some(server.url()),
        launcher_id: fixtures.launcher_id.clone(),
        max_nonce: 0,
        include_spent: false,
        asset_type: AssetTypeFilter::All,
        requested_cat_ids: HashSet::new(),
        requested_cat_tickers: Vec::new(),
        checkpoint_file: None,
        checkpoint_save_interval: 1,
        no_resume_checkpoint: false,
        nonce_batch_size: 32,
        empty_batch_stop_count: 1,
        parent_lookup_batch_size: 64,
        start_height: None,
        end_height: Some(100),
        incremental_from_checkpoint: false,
        auto_increment: false,
        cats_config: dir.path().join("missing-cats.yaml"),
        markets_config: dir.path().join("missing-markets.yaml"),
        testnet_markets_config: None,
        cache_clear: None,
    };

    let result = ScanState::run(request)
        .await
        .expect("scan should classify cat");
    assert_eq!(result.count, 1);
    assert_eq!(result.coins.len(), 1);
    assert_eq!(result.coins[0].kind, CoinKind::Cat);
    assert_eq!(
        result.coins[0].cat_asset_id.as_deref(),
        Some(fixtures.cat_asset_id.as_str())
    );
    assert_eq!(
        result.coins[0].coin_id,
        coin_id_from_record(&coin.cat_coin_record)
    );
}
