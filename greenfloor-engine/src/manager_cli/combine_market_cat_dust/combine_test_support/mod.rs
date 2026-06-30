mod execute;
mod sim;

pub(super) use execute::{
    dust_combine_batch_from_ids, ok_mixed_split_result, proven_dust, sample_combine_batch_plan,
};
pub(super) use sim::{dust_plan_from_scan_without_lineage, register_lineage_mocks_for_scan_coins};

use super::jobs::CatDustJob;
use crate::coinset::{resolve_coinset_endpoint, ResolvedCoinsetEndpoint};

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

pub(super) fn test_coinset_endpoint() -> ResolvedCoinsetEndpoint {
    resolve_coinset_endpoint("mainnet", "https://api.coinset.org", None)
}
