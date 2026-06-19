//! Shared simulator-backed CAT scan mocks for integration tests.

#![cfg(test)]

use std::path::Path;

use chia_protocol::CoinSpend;
use mockito::Matcher;
use serde_json::{json, Value};

use crate::coinset::{coin_id_from_record, to_coinset_hex};
use crate::hex::normalize_hex_id;
use crate::minimal_program_template::{materialize_minimal_program_text, MinimalProgramParams};
use crate::test_support::simulator::harness::SimulatorVaultHarness;
use crate::vault::members::hex_to_bytes32;

const MINIMAL_PROGRAM_SIGNER_APPEND: &str =
    include_str!("../../../tests/fixtures/data/minimal_program_signer_append.yaml");

const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

#[derive(Debug, Clone)]
pub struct CatCoinScanFixture {
    pub cat_coin_record: Value,
    pub cat_coin_name: String,
    pub parent_coin_record: Value,
    pub parent_coin_name: String,
    pub parent_spent_height: u64,
    pub puzzle_and_solution: Value,
}

#[derive(Debug, Clone)]
pub struct MultiCatScanFixtures {
    pub launcher_id: String,
    pub cat_asset_id: String,
    pub coins: Vec<CatCoinScanFixture>,
}

fn cat_coin_fixture(
    harness: &SimulatorVaultHarness,
    cat: &chia_sdk_driver::Cat,
) -> CatCoinScanFixture {
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

    CatCoinScanFixture {
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
    }
}

pub fn build_multi_dust_cat_scan_fixtures(amounts: &[u64]) -> MultiCatScanFixtures {
    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let launcher_id = normalize_hex_id(&hex::encode(harness.chain.launcher_id));
    let mut coins = Vec::new();
    for &amount in amounts {
        let cat = harness.fund_vault_cat(amount);
        coins.push(cat_coin_fixture(&harness, &cat));
    }
    let cat_asset_id = normalize_hex_id(&hex::encode(harness.chain.asset_id));
    MultiCatScanFixtures {
        launcher_id,
        cat_asset_id,
        coins,
    }
}

pub async fn mount_cat_scan_mocks(
    server: &mut mockito::ServerGuard,
    fixtures: &MultiCatScanFixtures,
) {
    let _state_mock = server
        .mock("POST", "/get_blockchain_state")
        .with_status(200)
        .with_body(r#"{"success":true,"blockchain_state":{"peak_height":100}}"#)
        .create_async()
        .await;

    let coin_records: Vec<_> = fixtures
        .coins
        .iter()
        .map(|coin| coin.cat_coin_record.clone())
        .collect();
    let coin_records_body = json!({
        "success": true,
        "coin_records": coin_records,
    })
    .to_string();

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

    for coin in &fixtures.coins {
        let parent_records_body = json!({
            "success": true,
            "coin_records": [coin.parent_coin_record.clone()],
        })
        .to_string();
        let _parent_mock = server
            .mock("POST", "/get_coin_records_by_names")
            .match_body(Matcher::PartialJson(json!({
                "names": [coin.parent_coin_name.clone()],
                "include_spent_coins": true,
            })))
            .with_status(200)
            .with_body(parent_records_body)
            .create_async()
            .await;
        let puzzle_solution_body = json!({
            "success": true,
            "coin_solution": coin.puzzle_and_solution.clone(),
        })
        .to_string();
        let _solution_mock = server
            .mock("POST", "/get_puzzle_and_solution")
            .match_body(Matcher::PartialJson(json!({
                "coin_id": coin.parent_coin_name.clone(),
                "height": coin.parent_spent_height,
            })))
            .with_status(200)
            .with_body(puzzle_solution_body)
            .create_async()
            .await;
    }

    let verify_body = json!({
        "success": true,
        "coin_records": coin_records,
    })
    .to_string();
    if fixtures.coins.len() == 1 {
        let coin = &fixtures.coins[0];
        let _verify_mock = server
            .mock("POST", "/get_coin_records_by_names")
            .match_body(Matcher::PartialJson(json!({
                "names": [coin.cat_coin_name.clone()],
                "include_spent_coins": true,
            })))
            .with_status(200)
            .with_body(
                json!({
                    "success": true,
                    "coin_records": [coin.cat_coin_record.clone()],
                })
                .to_string(),
            )
            .create_async()
            .await;
    } else {
        let _verify_mock = server
            .mock("POST", "/get_coin_records_by_names")
            .match_body(Matcher::Regex(
                r#""names"\s*:\s*\[[^\]]+,[^\]]+\]"#.to_string(),
            ))
            .with_status(200)
            .with_body(verify_body)
            .create_async()
            .await;
    }
}

pub fn write_combine_dust_test_configs(dir: &Path, fixtures: &MultiCatScanFixtures) {
    let program = dir.join("program.yaml");
    let markets = dir.join("markets.yaml");
    let cats = dir.join("cats.yaml");
    let mut program_text = materialize_minimal_program_text(MinimalProgramParams {
        home_dir: dir,
        ..Default::default()
    });
    program_text.push('\n');
    program_text
        .push_str(&MINIMAL_PROGRAM_SIGNER_APPEND.replace("__LAUNCHER_ID__", &fixtures.launcher_id));
    std::fs::write(&program, program_text).expect("write program");
    std::fs::write(
        &markets,
        format!(
            r#"markets:
  - id: dust_m
    enabled: true
    base_asset: "{cat_hex}"
    base_symbol: DUST
    quote_asset: xch
    quote_asset_type: unstable
    signer_key_id: key-main-1
    receive_address: {receive}
    mode: sell_only
    inventory:
      low_watermark_base_units: 100
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
"#,
            cat_hex = fixtures.cat_asset_id,
            receive = RECEIVE_ADDRESS,
        ),
    )
    .expect("write markets");
    std::fs::write(
        &cats,
        format!(
            "cats:\n  - base_symbol: DUST\n    asset_id: \"{}\"\n",
            fixtures.cat_asset_id
        ),
    )
    .expect("write cats");
}

pub fn expected_dust_coin_ids(fixtures: &MultiCatScanFixtures) -> Vec<String> {
    fixtures
        .coins
        .iter()
        .map(|coin| coin_id_from_record(&coin.cat_coin_record))
        .collect()
}

pub fn assert_unified_dry_run_batches(payload: &Value) {
    let jobs = payload
        .get("jobs")
        .and_then(Value::as_array)
        .expect("jobs array");
    assert_eq!(jobs.len(), 1);
    let job = &jobs[0];
    assert_eq!(job.get("status"), Some(&json!("ok")));
    assert_eq!(job.get("dust_coin_count"), Some(&json!(1)));
    assert_eq!(job.get("combine_batches_planned"), Some(&json!(0)));
    assert_eq!(job.get("uncombinable_dust_count"), Some(&json!(1)));

    let batches = job
        .get("batches")
        .and_then(Value::as_array)
        .expect("batches array");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].get("status"), Some(&json!("orphan")));
    assert_eq!(
        batches[0]
            .get("coin_ids")
            .and_then(Value::as_array)
            .map(std::vec::Vec::len),
        Some(1)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_coinset_scan::request::{build_cat_dust_scan_request, CatDustScanParams};
    use crate::vault_coinset_scan::ScanState;

    #[tokio::test]
    async fn cat_dust_scan_request_finds_mocked_coins() {
        let fixtures = build_multi_dust_cat_scan_fixtures(&[500]);
        let dir = tempfile::tempdir().expect("tempdir");
        write_combine_dust_test_configs(dir.path(), &fixtures);

        let mut server = mockito::Server::new_async().await;
        mount_cat_scan_mocks(&mut server, &fixtures).await;

        let request = build_cat_dust_scan_request(&CatDustScanParams {
            network: "mainnet",
            coinset_base_url: Some(&server.url()),
            launcher_id: &fixtures.launcher_id,
            max_nonce: 0,
            cat_asset_id: &fixtures.cat_asset_id,
            cats_config: &dir.path().join("cats.yaml"),
            markets_config: &dir.path().join("markets.yaml"),
            testnet_markets_config: None,
        });
        let result = ScanState::run(request)
            .await
            .expect("cat dust scan should succeed");
        assert_eq!(result.count, 1, "result: {result:?}");
    }

    #[tokio::test]
    async fn cat_dust_scan_request_finds_two_coin_preview_batch() {
        let fixtures = build_multi_dust_cat_scan_fixtures(&[400, 300]);
        let dir = tempfile::tempdir().expect("tempdir");
        write_combine_dust_test_configs(dir.path(), &fixtures);

        let mut server = mockito::Server::new_async().await;
        mount_cat_scan_mocks(&mut server, &fixtures).await;

        let mut request = build_cat_dust_scan_request(&CatDustScanParams {
            network: "mainnet",
            coinset_base_url: Some(&server.url()),
            launcher_id: &fixtures.launcher_id,
            max_nonce: 0,
            cat_asset_id: &fixtures.cat_asset_id,
            cats_config: &dir.path().join("cats.yaml"),
            markets_config: &dir.path().join("markets.yaml"),
            testnet_markets_config: None,
        });
        request.requested_cat_ids.clear();
        request.asset_type = crate::vault_coinset_scan::types::AssetTypeFilter::All;
        let result = ScanState::run(request)
            .await
            .expect("two-coin scan should succeed");
        assert_eq!(result.count, 2, "result: {result:?}");
    }
}
