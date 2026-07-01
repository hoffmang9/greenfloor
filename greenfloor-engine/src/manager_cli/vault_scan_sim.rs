//! Simulator-backed vault scan fixtures shared by dust and trace tests.

use chia_sdk_driver::Cat;

use crate::hex::normalize_hex_id;
use crate::test_support::simulator::harness::SimulatorVaultHarness;
use crate::vault_coinset_scan::result::{
    CheckpointSummary, ScanBatchConfig, ScanResult, ScanWindowSummary,
};
use crate::vault_coinset_scan::types::{AssetTypeFilter, CoinKind, CoinRow, ScanStopReason};

pub fn coin_row_from_sim_cat(cat: &Cat, asset_id_hex: &str) -> CoinRow {
    CoinRow {
        coin_id: normalize_hex_id(&hex::encode(cat.coin.coin_id())),
        puzzle_hash: hex::encode(cat.coin.puzzle_hash),
        parent_coin_info: hex::encode(cat.coin.parent_coin_info),
        amount: cat.coin.amount,
        confirmed_block_index: 12,
        spent_block_index: 0,
        discovered_nonces: vec![0],
        discovered_by_puzzle_hash: true,
        discovered_by_hint: false,
        kind: CoinKind::Cat,
        cat_asset_id: Some(asset_id_hex.to_string()),
        cat_symbols: Vec::new(),
    }
}

pub fn scan_result_from_coin_rows(coins: Vec<CoinRow>, launcher_id: &str) -> ScanResult {
    let count = coins.len();
    ScanResult {
        network: "mainnet".to_string(),
        coinset_base_url: None,
        launcher_id: launcher_id.to_string(),
        asset_type: AssetTypeFilter::Cat,
        requested_cat_ids: Vec::new(),
        requested_cat_tickers: Vec::new(),
        max_nonce_scanned: 0,
        count,
        name_verification: None,
        cache_clear: None,
        checkpoint: CheckpointSummary {
            enabled: false,
            file: None,
            resumed: false,
            start_nonce: 0,
            save_interval: None,
            cat_asset_cache_entries: 0,
            parent_lineage_cache_entries: 0,
            last_synced_height: None,
            discard_reason: None,
        },
        scan_batches: ScanBatchConfig {
            nonce_batch_size: 32,
            empty_batch_stop_count: 1,
            parent_lookup_batch_size: 64,
        },
        scan_window: ScanWindowSummary {
            start_height: None,
            end_height: None,
            chain_peak_height: None,
            incremental_from_checkpoint: false,
            auto_increment: false,
        },
        scan_stop_reason: ScanStopReason::MaxNonceReached,
        coins,
    }
}

pub fn sim_dust_scan_result(amounts: &[u64]) -> (ScanResult, SimulatorVaultHarness) {
    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let launcher_id = normalize_hex_id(&hex::encode(harness.chain.launcher_id));
    let asset_id = normalize_hex_id(&hex::encode(harness.chain.asset_id));
    let rows: Vec<CoinRow> = amounts
        .iter()
        .map(|&amount| {
            let cat = harness.fund_vault_cat(amount);
            coin_row_from_sim_cat(&cat, &asset_id)
        })
        .collect();
    (scan_result_from_coin_rows(rows, &launcher_id), harness)
}
