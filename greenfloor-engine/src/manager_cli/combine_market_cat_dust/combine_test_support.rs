use super::coinset_context::resolve_combine_coinset_context;
use super::jobs::CatDustJob;
use crate::hex::hex_to_bytes32;
use crate::test_support::simulator::harness::{fetch_cat_from_sim_by_id, SimulatorVaultHarness};
use crate::vault_coinset_scan::{
    dust_coins_from_scan, plan_dust_batches, DustPlan, ProvenDustCoin, ScanResult,
};

pub(super) const RECEIVE_ADDRESS: &str =
    "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

pub(super) fn sample_job(cat_asset_id: &str) -> CatDustJob {
    CatDustJob {
        cat_asset_id: cat_asset_id.to_string(),
        signer_key_id: "key-main-1".to_string(),
        receive_address: RECEIVE_ADDRESS.to_string(),
        market_ids: vec!["dust_m".to_string()],
    }
}

pub(super) fn test_coinset_context() -> super::coinset_context::CombineCoinsetContext {
    resolve_combine_coinset_context(None, None, "mainnet", "https://api.coinset.org")
}

pub(super) fn dust_plan_from_scan_without_lineage(
    scan: &ScanResult,
    harness: &SimulatorVaultHarness,
    dust_threshold_mojos: u64,
    max_input_coins: usize,
) -> DustPlan {
    let dust = dust_coins_from_scan(&scan.coins, dust_threshold_mojos);
    let proven: Vec<ProvenDustCoin> = dust
        .iter()
        .map(|coin| {
            let coin_id = hex_to_bytes32(&coin.coin_id).expect("coin id");
            let cat = fetch_cat_from_sim_by_id(&harness.chain, coin_id).expect("sim cat");
            ProvenDustCoin::new(coin.clone(), cat).expect("proven dust")
        })
        .collect();
    DustPlan {
        scan_dust_count: dust.len(),
        batches: plan_dust_batches(&proven, max_input_coins),
        lineage_excluded: Vec::new(),
    }
}

pub(super) fn register_lineage_mocks_for_scan_coins(
    server: &mut mockito::ServerGuard,
    scan: &ScanResult,
    harness: &crate::test_support::simulator::harness::SimulatorVaultHarness,
) {
    use crate::coinset::test_support::{
        coin_record_by_name_request_json, mock_get_coin_record_by_name_body,
        mock_get_puzzle_and_solution_body, mock_unspent_coin_record_by_name_body,
    };
    use crate::test_support::simulator::harness::fetch_cat_from_sim;
    use chia_protocol::CoinSpend;
    use mockito::Matcher;

    let sim = harness.chain.sim.lock().expect("sim lock");
    for row in &scan.coins {
        let coin_id = hex_to_bytes32(&row.coin_id).expect("coin id");
        let coin = sim
            .coin_state(coin_id)
            .map(|state| state.coin)
            .expect("coin state");
        let cat = fetch_cat_from_sim(&sim, coin).expect("sim cat");
        server
            .mock("POST", "/get_coin_record_by_name")
            .match_body(Matcher::PartialJson(coin_record_by_name_request_json(
                cat.coin.coin_id(),
            )))
            .with_status(200)
            .with_body(mock_unspent_coin_record_by_name_body(&cat.coin))
            .create();
        let parent = sim
            .coin_spend(cat.coin.parent_coin_info)
            .expect("parent spend");
        let spent_height = sim
            .coin_state(parent.coin.coin_id())
            .and_then(|state| state.spent_height)
            .unwrap_or(1);
        server
            .mock("POST", "/get_coin_record_by_name")
            .match_body(Matcher::PartialJson(coin_record_by_name_request_json(
                parent.coin.coin_id(),
            )))
            .with_status(200)
            .with_body(mock_get_coin_record_by_name_body(
                &parent.coin,
                spent_height,
            ))
            .create();
        let parent_spend = CoinSpend {
            coin: parent.coin,
            puzzle_reveal: parent.puzzle_reveal.clone(),
            solution: parent.solution.clone(),
        };
        server
            .mock("POST", "/get_puzzle_and_solution")
            .with_status(200)
            .with_body(mock_get_puzzle_and_solution_body(&parent_spend))
            .create();
    }
}
