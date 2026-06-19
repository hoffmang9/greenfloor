use chia_protocol::CoinSpend;

use crate::coinset::child_cat_asset_ids_from_parent_spend;
use crate::hex::normalize_hex_id;
use crate::test_support::simulator::harness::SimulatorVaultHarness;
use crate::vault_coinset_scan::types::CoinKind;

#[test]
fn simulator_cat_parent_spend_classifies_child_asset_id() {
    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let cat = harness.fund_vault_cat(5_000);
    let cat_coin_id = normalize_hex_id(&hex::encode(cat.coin.coin_id()));
    let asset_id = normalize_hex_id(&hex::encode(harness.chain.asset_id));

    let sim = harness.chain.sim.lock().expect("sim lock");
    let parent = sim
        .coin_spend(cat.coin.parent_coin_info)
        .expect("parent spend");
    let parent_spend = CoinSpend {
        coin: parent.coin,
        puzzle_reveal: parent.puzzle_reveal.clone(),
        solution: parent.solution.clone(),
    };
    drop(sim);

    let child_assets =
        child_cat_asset_ids_from_parent_spend(parent.coin, &parent_spend).expect("child assets");
    let asset = child_assets
        .get(&cat_coin_id)
        .expect("classified cat coin id");
    assert_eq!(asset, &asset_id);
    assert!(CoinKind::Cat.is_cat());
}
