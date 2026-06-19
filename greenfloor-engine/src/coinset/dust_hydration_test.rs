//! Scan-derived dust coin IDs hydrate to spendable CATs via Coinset lookup.

use chia_protocol::{Coin, CoinSpend};
use chia_sdk_coinset::CoinsetClient;
use chia_sdk_driver::Cat;
use mockito::Matcher;
use serde_json::{json, Value};

use crate::coinset::{select_cats_for_spend, to_coinset_hex};
use crate::hex::normalize_hex_id;
use crate::test_support::simulator::harness::SimulatorVaultHarness;
use crate::vault::members::hex_to_bytes32;
use crate::vault_coinset_scan::types::{CoinKind, CoinRow};
use crate::vault_coinset_scan::{dust_coins_from_scan, plan_dust_batches};

const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

struct DustCatFixtures {
    cat_coin_name: String,
    parent_coin_name: String,
    cat_record_response: String,
    parent_record_response: String,
    puzzle_solution_response: String,
    cat_asset_id: chia_protocol::Bytes32,
    coin_row: CoinRow,
}

fn coin_json(coin: Coin) -> Value {
    json!({
        "parent_coin_info": format!("0x{}", hex::encode(coin.parent_coin_info)),
        "puzzle_hash": format!("0x{}", hex::encode(coin.puzzle_hash)),
        "amount": coin.amount,
    })
}

fn coin_record_response(
    coin: Coin,
    confirmed_block_index: u32,
    spent: bool,
    spent_block_index: u32,
) -> String {
    json!({
        "success": true,
        "coin_record": {
            "coin": coin_json(coin),
            "coinbase": false,
            "confirmed_block_index": confirmed_block_index,
            "spent": spent,
            "spent_block_index": spent_block_index,
            "timestamp": 1,
        },
    })
    .to_string()
}

fn puzzle_solution_response(spend: &CoinSpend) -> String {
    json!({
        "success": true,
        "coin_solution": {
            "coin": coin_json(spend.coin),
            "puzzle_reveal": format!("0x{}", hex::encode(spend.puzzle_reveal.as_ref())),
            "solution": format!("0x{}", hex::encode(spend.solution.as_ref())),
        },
    })
    .to_string()
}

fn build_dust_cat_fixtures(amount: u64) -> DustCatFixtures {
    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let cat = harness.fund_vault_cat(amount);
    build_dust_cat_fixtures_from_cat(&cat, &harness)
}

fn build_dust_cat_fixtures_from_cat(cat: &Cat, harness: &SimulatorVaultHarness) -> DustCatFixtures {
    let cat_coin_id = hex::encode(cat.coin.coin_id());
    let parent_coin_id = cat.coin.parent_coin_info;
    let sim = harness.chain.sim.lock().expect("sim lock");
    let parent_spend = sim
        .coin_spend(parent_coin_id)
        .expect("parent spend for cat");
    let parent_state = sim.coin_state(parent_coin_id).expect("parent coin state");
    drop(sim);

    let parent_spent_height = parent_state.spent_height.unwrap_or(1);
    let parent_confirmed_height = parent_state.created_height.unwrap_or(1);
    let parent_spend = CoinSpend {
        coin: parent_spend.coin,
        puzzle_reveal: parent_spend.puzzle_reveal.clone(),
        solution: parent_spend.solution.clone(),
    };
    let asset_id = cat.info.asset_id;
    let coin_row = CoinRow {
        coin_id: normalize_hex_id(&cat_coin_id),
        puzzle_hash: hex::encode(cat.coin.puzzle_hash),
        parent_coin_info: hex::encode(cat.coin.parent_coin_info),
        amount: cat.coin.amount,
        confirmed_block_index: 12,
        spent_block_index: 0,
        discovered_nonces: vec![0],
        discovered_by_puzzle_hash: true,
        discovered_by_hint: false,
        kind: CoinKind::Cat,
        cat_asset_id: Some(normalize_hex_id(&hex::encode(asset_id))),
        cat_symbols: vec![],
    };

    DustCatFixtures {
        cat_coin_name: to_coinset_hex(hex_to_bytes32(&cat_coin_id).expect("cat id").as_ref()),
        parent_coin_name: to_coinset_hex(
            hex_to_bytes32(&hex::encode(parent_coin_id))
                .expect("parent id")
                .as_ref(),
        ),
        cat_record_response: coin_record_response(cat.coin, 12, false, 0),
        parent_record_response: coin_record_response(
            parent_state.coin,
            parent_confirmed_height,
            true,
            parent_spent_height,
        ),
        puzzle_solution_response: puzzle_solution_response(&parent_spend),
        cat_asset_id: asset_id,
        coin_row,
    }
}

async fn mount_cat_hydration_mocks(server: &mut mockito::ServerGuard, fixtures: &DustCatFixtures) {
    let _cat_mock = server
        .mock("POST", "/get_coin_record_by_name")
        .match_body(Matcher::PartialJson(json!({
            "name": fixtures.cat_coin_name.clone(),
        })))
        .with_status(200)
        .with_body(fixtures.cat_record_response.clone())
        .create_async()
        .await;
    let _parent_mock = server
        .mock("POST", "/get_coin_record_by_name")
        .match_body(Matcher::PartialJson(json!({
            "name": fixtures.parent_coin_name.clone(),
        })))
        .with_status(200)
        .with_body(fixtures.parent_record_response.clone())
        .create_async()
        .await;
    let _solution_mock = server
        .mock("POST", "/get_puzzle_and_solution")
        .match_body(Matcher::PartialJson(json!({
            "coin_id": fixtures.parent_coin_name.clone(),
        })))
        .with_status(200)
        .with_body(fixtures.puzzle_solution_response.clone())
        .create_async()
        .await;
}

#[tokio::test]
async fn scan_dust_coin_ids_hydrate_via_select_cats_for_spend() {
    let fixtures = build_dust_cat_fixtures(500);
    let dust = dust_coins_from_scan(std::slice::from_ref(&fixtures.coin_row), 1000);
    assert_eq!(dust.len(), 1);

    let mut server = mockito::Server::new_async().await;
    mount_cat_hydration_mocks(&mut server, &fixtures).await;

    let client = CoinsetClient::new(server.url());
    let coin_id = hex_to_bytes32(&dust[0].coin_id).expect("dust coin id");
    let selected = select_cats_for_spend(
        &client,
        RECEIVE_ADDRESS,
        fixtures.cat_asset_id,
        std::slice::from_ref(&coin_id),
        dust[0].amount,
    )
    .await
    .expect("hydrate scan dust coin id");
    assert_eq!(selected.selected.len(), 1);
    assert_eq!(selected.offered_total, dust[0].amount);
    assert_eq!(selected.change_amount, 0);
}

#[tokio::test]
async fn scan_dust_batch_hydrates_for_mixed_split_target() {
    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let cat_left = harness.fund_vault_cat(400);
    let left = build_dust_cat_fixtures_from_cat(&cat_left, &harness);
    let cat_right = harness.fund_vault_cat(300);
    let right = build_dust_cat_fixtures_from_cat(&cat_right, &harness);

    let rows = vec![left.coin_row.clone(), right.coin_row.clone()];
    let dust = dust_coins_from_scan(&rows, 1000);
    let plan = plan_dust_batches(&dust, 2);
    assert_eq!(plan.combinable_batches.len(), 1);
    assert_eq!(plan.combinable_batches[0].len(), 2);

    let mut server = mockito::Server::new_async().await;
    mount_cat_hydration_mocks(&mut server, &left).await;
    mount_cat_hydration_mocks(&mut server, &right).await;

    let client = CoinsetClient::new(server.url());
    let coin_ids: Vec<_> = plan.combinable_batches[0]
        .iter()
        .map(|coin| hex_to_bytes32(&coin.coin_id).expect("coin id"))
        .collect();
    let target_total: u64 = plan.combinable_batches[0]
        .iter()
        .map(|coin| coin.amount)
        .sum();
    let selected = select_cats_for_spend(
        &client,
        RECEIVE_ADDRESS,
        left.cat_asset_id,
        &coin_ids,
        target_total,
    )
    .await
    .expect("hydrate dust batch");
    assert_eq!(selected.selected.len(), 2);
    assert_eq!(selected.offered_total, target_total);
    assert_eq!(selected.change_amount, 0);
}
