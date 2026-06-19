use std::collections::HashSet;

use chia_protocol::CoinSpend;
use mockito::Matcher;
use serde_json::json;

use crate::coinset::{coin_id_from_record, to_coinset_hex};
use crate::hex::normalize_hex_id;
use crate::test_support::simulator::harness::SimulatorVaultHarness;
use crate::vault::members::hex_to_bytes32;
use crate::vault_coinset_scan::request::ScanRequest;
use crate::vault_coinset_scan::types::{AssetTypeFilter, CoinKind};

use super::ScanState;

struct CatScanFixtures {
    launcher_id: String,
    cat_coin_record: serde_json::Value,
    cat_coin_name: String,
    parent_coin_record: serde_json::Value,
    parent_coin_name: String,
    parent_spent_height: u64,
    puzzle_and_solution: serde_json::Value,
    cat_asset_id: String,
}

fn build_cat_scan_fixtures() -> CatScanFixtures {
    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let cat = harness.fund_vault_cat(5_000);
    let launcher_id = hex::encode(harness.chain.launcher_id);
    let cat_coin_id = hex::encode(cat.coin.coin_id());
    let parent_coin_id = cat.coin.parent_coin_info;
    let sim = harness.chain.sim.lock().expect("sim lock");
    let parent_spend = sim
        .coin_spend(parent_coin_id)
        .expect("parent spend for cat");
    let parent_state = sim.coin_state(parent_coin_id).expect("parent coin state");
    drop(sim);

    let parent_spent_height = u64::from(parent_state.spent_height.unwrap_or(1));
    let parent_confirmed_height = u64::from(parent_state.created_height.unwrap_or(1));
    let parent_coin_record = json!({
        "coin": {
            "name": hex::encode(parent_coin_id),
            "parent_coin_info": hex::encode(parent_state.coin.parent_coin_info),
            "puzzle_hash": hex::encode(parent_state.coin.puzzle_hash),
            "amount": parent_state.coin.amount,
        },
        "confirmed_block_index": parent_confirmed_height,
        "spent_block_index": parent_spent_height,
    });
    let cat_coin_record = json!({
        "coin": {
            "name": cat_coin_id.clone(),
            "parent_coin_info": hex::encode(cat.coin.parent_coin_info),
            "puzzle_hash": hex::encode(cat.coin.puzzle_hash),
            "amount": cat.coin.amount,
        },
        "confirmed_block_index": 12,
        "spent_block_index": 0,
    });
    let parent_spend = CoinSpend {
        coin: parent_spend.coin,
        puzzle_reveal: parent_spend.puzzle_reveal.clone(),
        solution: parent_spend.solution.clone(),
    };
    let puzzle_and_solution = json!({
        "puzzle_reveal": format!("0x{}", hex::encode(parent_spend.puzzle_reveal.as_ref())),
        "solution": format!("0x{}", hex::encode(parent_spend.solution.as_ref())),
    });

    CatScanFixtures {
        launcher_id: normalize_hex_id(&launcher_id),
        cat_coin_name: to_coinset_hex(hex_to_bytes32(&cat_coin_id).expect("cat id").as_ref()),
        parent_coin_name: to_coinset_hex(
            hex_to_bytes32(&hex::encode(parent_coin_id))
                .expect("parent id")
                .as_ref(),
        ),
        cat_coin_record,
        parent_coin_record,
        parent_spent_height,
        puzzle_and_solution,
        cat_asset_id: normalize_hex_id(&hex::encode(cat.info.asset_id)),
    }
}

#[tokio::test]
async fn run_vault_coinset_scan_discovers_cat_via_parent_spend() {
    let fixtures = build_cat_scan_fixtures();
    let coin_records_body = json!({
        "success": true,
        "coin_records": [fixtures.cat_coin_record.clone()],
    })
    .to_string();
    let parent_records_body = json!({
        "success": true,
        "coin_records": [fixtures.parent_coin_record.clone()],
    })
    .to_string();
    let puzzle_solution_body = json!({
        "success": true,
        "coin_solution": fixtures.puzzle_and_solution.clone(),
    })
    .to_string();

    let mut server = mockito::Server::new_async().await;
    let _puzzle_mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hashes")
        .with_status(200)
        .with_body(coin_records_body.clone())
        .create_async()
        .await;
    let _hint_mock = server
        .mock("POST", "/get_coin_records_by_hints")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[]}"#)
        .create_async()
        .await;
    let _parent_mock = server
        .mock("POST", "/get_coin_records_by_names")
        .match_body(Matcher::PartialJson(json!({
            "names": [fixtures.parent_coin_name.clone()],
            "include_spent_coins": true,
        })))
        .with_status(200)
        .with_body(parent_records_body)
        .create_async()
        .await;
    let _verify_mock = server
        .mock("POST", "/get_coin_records_by_names")
        .match_body(Matcher::PartialJson(json!({
            "names": [fixtures.cat_coin_name.clone()],
            "include_spent_coins": true,
        })))
        .with_status(200)
        .with_body(coin_records_body)
        .create_async()
        .await;
    let _solution_mock = server
        .mock("POST", "/get_puzzle_and_solution")
        .match_body(Matcher::PartialJson(json!({
            "coin_id": fixtures.parent_coin_name.clone(),
            "height": fixtures.parent_spent_height,
        })))
        .with_status(200)
        .with_body(puzzle_solution_body)
        .create_async()
        .await;

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
        coin_id_from_record(&fixtures.cat_coin_record)
    );
}
