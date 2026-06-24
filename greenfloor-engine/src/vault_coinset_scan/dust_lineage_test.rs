//! Lineage integration tests for [`super::prove_dust_coins_lineage`].

use super::{prove_dust_coins_lineage, DustCoin};
use crate::coinset::test_support::{
    coin_record_by_name_request_json, mock_get_coin_record_by_name_body,
    mock_get_puzzle_and_solution_body, mock_unspent_coin_record_by_name_body,
};
use crate::hex::{hex_to_bytes32, normalize_hex_id};
use crate::test_support::simulator::harness::SimulatorVaultHarness;
use chia_protocol::{Bytes32, CoinSpend};
use chia_sdk_coinset::CoinsetClient;
use mockito::Matcher;
use serde_json::json;

#[tokio::test]
async fn prove_dust_coins_lineage_excludes_unresolvable_parent() {
    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let good_cat = harness.fund_vault_cat(400);
    let bad_coin_id = normalize_hex_id(&hex::encode(Bytes32::new([0xcc; 32])));
    let bad_coin_name = hex_to_bytes32(&bad_coin_id).expect("bad coin id");
    let dust = vec![
        DustCoin {
            coin_id: normalize_hex_id(&hex::encode(good_cat.coin.coin_id())),
            amount: good_cat.coin.amount,
        },
        DustCoin {
            coin_id: bad_coin_id.clone(),
            amount: 300,
        },
    ];

    let (parent_body, puzzle_body, parent_coin_id) = {
        let sim = harness.chain.sim.lock().expect("sim lock");
        let parent = sim
            .coin_spend(good_cat.coin.parent_coin_info)
            .expect("parent spend");
        let spent_height = sim
            .coin_state(parent.coin.coin_id())
            .and_then(|state| state.spent_height)
            .unwrap_or(1);
        let parent_spend = CoinSpend {
            coin: parent.coin,
            puzzle_reveal: parent.puzzle_reveal.clone(),
            solution: parent.solution.clone(),
        };
        (
            mock_get_coin_record_by_name_body(&parent.coin, spent_height),
            mock_get_puzzle_and_solution_body(&parent_spend),
            parent.coin.coin_id(),
        )
    };

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/get_coin_record_by_name")
        .match_body(Matcher::PartialJson(coin_record_by_name_request_json(
            good_cat.coin.coin_id(),
        )))
        .with_status(200)
        .with_body(mock_unspent_coin_record_by_name_body(&good_cat.coin))
        .create();
    server
        .mock("POST", "/get_coin_record_by_name")
        .match_body(Matcher::PartialJson(coin_record_by_name_request_json(
            bad_coin_name,
        )))
        .with_status(200)
        .with_body(json!({"success": true, "coin_record": null}).to_string())
        .create();
    server
        .mock("POST", "/get_coin_record_by_name")
        .match_body(Matcher::PartialJson(coin_record_by_name_request_json(
            parent_coin_id,
        )))
        .with_status(200)
        .with_body(parent_body)
        .create();
    server
        .mock("POST", "/get_puzzle_and_solution")
        .with_status(200)
        .with_body(puzzle_body)
        .create();

    let client = CoinsetClient::new(server.url());
    let (proven, excluded) = prove_dust_coins_lineage(&client, &dust)
        .await
        .expect("filter");
    assert_eq!(proven.len(), 1);
    assert_eq!(proven[0].dust_coin().coin_id, dust[0].coin_id);
    assert_eq!(proven[0].cat().coin.amount, good_cat.coin.amount);
    assert_eq!(excluded.len(), 1);
    assert_eq!(excluded[0].coin_id, bad_coin_id);
}
