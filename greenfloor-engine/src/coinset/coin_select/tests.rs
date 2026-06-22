use chia_protocol::{Bytes32, CoinSpend};
use chia_sdk_coinset::CoinsetClient;
use chia_sdk_test::Simulator;

use super::{finalize_selected_cats, select_cats_for_spend, select_from_list, CoinSelectionMode};
use crate::coinset::test_support::{
    cat_with_amount, mock_get_coin_record_by_name_body, mock_get_coin_records_by_puzzle_hash_body,
    mock_get_puzzle_and_solution_body,
};
use crate::error::SignerError;
use crate::test_support::simulator::harness::SimulatorVaultHarness;

fn parent_spent_block_index(sim: &Simulator, parent_coin_id: Bytes32) -> u32 {
    sim.coin_state(parent_coin_id)
        .and_then(|state| state.spent_height)
        .unwrap_or(1)
}

fn simulator_receive_address(harness: &SimulatorVaultHarness) -> String {
    crate::bech32m::encode_address(harness.chain.p2_message_hash, "xch").expect("receive address")
}

#[test]
fn smallest_first_prefers_exact_single_coin() {
    let cats = vec![
        cat_with_amount(1000),
        cat_with_amount(1000),
        cat_with_amount(10_000),
        cat_with_amount(100_000),
    ];
    let selected = select_from_list(
        cats,
        10_000,
        CoinSelectionMode::SmallestFirst,
        |cat| cat.coin.amount,
        SignerError::NoUnspentCatCoins,
        SignerError::InsufficientCatCoins,
    )
    .expect("selection");
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].coin.amount, 10_000);
}

#[test]
fn smallest_first_prefers_smallest_single_cover_coin() {
    let cats = vec![
        cat_with_amount(1000),
        cat_with_amount(20_000),
        cat_with_amount(100_000),
    ];
    let selected = select_from_list(
        cats,
        10_000,
        CoinSelectionMode::SmallestFirst,
        |cat| cat.coin.amount,
        SignerError::NoUnspentCatCoins,
        SignerError::InsufficientCatCoins,
    )
    .expect("selection");
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].coin.amount, 20_000);
}

#[test]
fn smallest_first_accumulates_when_no_single_coin_covers_target() {
    let cats = vec![
        cat_with_amount(2000),
        cat_with_amount(1000),
        cat_with_amount(1500),
    ];
    let selected = select_from_list(
        cats,
        2500,
        CoinSelectionMode::SmallestFirst,
        |cat| cat.coin.amount,
        SignerError::NoUnspentCatCoins,
        SignerError::InsufficientCatCoins,
    )
    .expect("selection");
    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].coin.amount, 1000);
    assert_eq!(selected[1].coin.amount, 1500);
}

#[test]
fn smallest_first_empty_list_uses_empty_error() {
    use chia_sdk_driver::Cat;

    let err = select_from_list(
        Vec::<Cat>::new(),
        1000,
        CoinSelectionMode::SmallestFirst,
        |cat| cat.coin.amount,
        SignerError::NoUnspentCatCoins,
        SignerError::InsufficientCatCoins,
    )
    .expect_err("empty");
    assert!(matches!(err, SignerError::NoUnspentCatCoins));
}

#[test]
fn smallest_first_insufficient_uses_insufficient_error() {
    let err = select_from_list(
        vec![cat_with_amount(500)],
        1000,
        CoinSelectionMode::SmallestFirst,
        |cat| cat.coin.amount,
        SignerError::NoUnspentCatCoins,
        SignerError::InsufficientCatCoins,
    )
    .expect_err("insufficient");
    assert!(matches!(err, SignerError::InsufficientCatCoins));
}

#[test]
fn explicit_sum_requires_full_set_total() {
    let selected = select_from_list(
        vec![cat_with_amount(700), cat_with_amount(400)],
        1000,
        CoinSelectionMode::ExplicitSum,
        |cat| cat.coin.amount,
        SignerError::NoUnspentCatCoins,
        SignerError::InsufficientCatCoins,
    )
    .expect("sum covers target");
    assert_eq!(selected.len(), 2);
    assert_eq!(
        selected.iter().map(|cat| cat.coin.amount).sum::<u64>(),
        1100
    );
}

#[test]
fn explicit_sum_fails_when_total_below_target() {
    let err = select_from_list(
        vec![cat_with_amount(400)],
        1000,
        CoinSelectionMode::ExplicitSum,
        |cat| cat.coin.amount,
        SignerError::NoUnspentCatCoins,
        SignerError::InsufficientCatCoins,
    )
    .expect_err("below target");
    assert!(matches!(err, SignerError::InsufficientCatCoins));
}

#[test]
fn finalize_selected_cats_uses_explicit_sum_for_fixed_ids() {
    let cats = vec![cat_with_amount(600), cat_with_amount(500)];
    let selected = finalize_selected_cats(cats, &[Bytes32::new([0xab; 32])], 1000)
        .expect("vault-style explicit selection");
    assert_eq!(selected.selected.len(), 2);
    assert_eq!(selected.offered_total, 1100);
    assert_eq!(selected.change_amount, 100);
}

#[tokio::test]
async fn select_cats_for_spend_skips_unresolvable_coins_before_selection() {
    let mut harness = SimulatorVaultHarness::new();
    let _cat_small = harness.fund_vault_cat(2000);
    let _cat_target = harness.fund_vault_cat(10_000);
    let cat_large = harness.fund_vault_cat(50_000);
    let receive_address = simulator_receive_address(&harness);
    let asset_id = harness.chain.asset_id;
    let broken_parent = Bytes32::new([0xbb; 32]);
    let broken_coin = cat_large.coin;
    let broken_record = chia_sdk_coinset::CoinRecord {
        coin: chia_protocol::Coin {
            parent_coin_info: broken_parent,
            puzzle_hash: broken_coin.puzzle_hash,
            amount: 2000,
        },
        confirmed_block_index: 1,
        spent: false,
        coinbase: false,
        spent_block_index: 0,
        timestamp: 1,
    };
    let list_body =
        mock_get_coin_records_by_puzzle_hash_body(&[broken_record.coin, cat_large.coin]);

    let (parent_body, puzzle_body) = {
        let sim = harness.chain.sim.lock().expect("sim lock");
        let parent = sim
            .coin_spend(cat_large.coin.parent_coin_info)
            .expect("parent spend");
        let spent_block_index = parent_spent_block_index(&sim, parent.coin.coin_id());
        let parent_spend = CoinSpend {
            coin: parent.coin,
            puzzle_reveal: parent.puzzle_reveal.clone(),
            solution: parent.solution.clone(),
        };
        (
            mock_get_coin_record_by_name_body(&parent.coin, spent_block_index),
            mock_get_puzzle_and_solution_body(&parent_spend),
        )
    };

    let mut server = mockito::Server::new_async().await;
    let list_mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(list_body)
        .expect(1)
        .create_async()
        .await;
    let parent_mock = server
        .mock("POST", "/get_coin_record_by_name")
        .with_status(200)
        .with_body(parent_body)
        .expect(1)
        .create_async()
        .await;
    let puzzle_mock = server
        .mock("POST", "/get_puzzle_and_solution")
        .with_status(200)
        .with_body(puzzle_body)
        .expect(1)
        .create_async()
        .await;

    let client = CoinsetClient::new(server.url());
    let selected = select_cats_for_spend(&client, &receive_address, asset_id, &[], 12_000)
        .await
        .expect("skips broken lineage and selects spendable cover coin");
    assert_eq!(selected.selected.len(), 1);
    assert_eq!(
        selected.selected[0].coin.coin_id(),
        cat_large.coin.coin_id()
    );
    assert_eq!(selected.offered_total, 50_000);

    list_mock.assert_async().await;
    parent_mock.assert_async().await;
    puzzle_mock.assert_async().await;
}

#[tokio::test]
async fn select_cats_for_spend_resolves_lineage_happy_path_for_selected_coin() {
    let mut harness = SimulatorVaultHarness::new();
    let _cat_small = harness.fund_vault_cat(2000);
    let cat_target = harness.fund_vault_cat(10_000);
    let _cat_large = harness.fund_vault_cat(50_000);
    let receive_address = simulator_receive_address(&harness);
    let asset_id = harness.chain.asset_id;
    let list_body = mock_get_coin_records_by_puzzle_hash_body(&[cat_target.coin]);

    let (parent_body, puzzle_body) = {
        let sim = harness.chain.sim.lock().expect("sim lock");
        let parent = sim
            .coin_spend(cat_target.coin.parent_coin_info)
            .expect("parent spend");
        let spent_block_index = parent_spent_block_index(&sim, parent.coin.coin_id());
        let parent_spend = CoinSpend {
            coin: parent.coin,
            puzzle_reveal: parent.puzzle_reveal.clone(),
            solution: parent.solution.clone(),
        };
        (
            mock_get_coin_record_by_name_body(&parent.coin, spent_block_index),
            mock_get_puzzle_and_solution_body(&parent_spend),
        )
    };

    let mut server = mockito::Server::new_async().await;
    let list_mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(list_body)
        .expect(1)
        .create_async()
        .await;
    let parent_mock = server
        .mock("POST", "/get_coin_record_by_name")
        .with_status(200)
        .with_body(parent_body)
        .expect(1)
        .create_async()
        .await;
    let puzzle_mock = server
        .mock("POST", "/get_puzzle_and_solution")
        .with_status(200)
        .with_body(puzzle_body)
        .expect(1)
        .create_async()
        .await;

    let client = CoinsetClient::new(server.url());
    let selected = select_cats_for_spend(&client, &receive_address, asset_id, &[], 10_000)
        .await
        .expect("selection succeeds");
    assert_eq!(selected.selected.len(), 1);
    assert_eq!(
        selected.selected[0].coin.coin_id(),
        cat_target.coin.coin_id()
    );
    assert_eq!(selected.offered_total, 10_000);
    assert_eq!(selected.change_amount, 0);

    list_mock.assert_async().await;
    parent_mock.assert_async().await;
    puzzle_mock.assert_async().await;
}
