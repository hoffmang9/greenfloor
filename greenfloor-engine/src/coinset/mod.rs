mod api;
mod asset;
mod backend;
mod batch;
mod broadcast;
mod cats;
pub(crate) mod coin_select;
mod direct_api;
mod json_util;
mod offer_assets;
mod pagination;
mod parse;
mod poll;
mod presplit;
pub mod probe;
mod retry;
mod rpc_result;
mod scan_client;
mod signer_client;
mod spent_verify;
#[cfg(test)]
pub(crate) mod test_support;
mod vault_fetch;
mod wallet_io;
mod xch;

pub use api::{
    conservative_fee_from_payload, direct_coinset_client, get_all_mempool_tx_ids,
    get_conservative_fee_estimate, get_conservative_fee_estimate_for_signer, get_fee_estimate,
    post_coinset_coin_records, post_coinset_record, post_coinset_rpc, push_tx_hex,
};
pub use asset::is_canonical_xch_asset;
pub use asset::is_xch_like_asset;
pub use backend::{LiveCoinset, OfferCoinsetBackend};
pub use batch::chunk_values;
pub use broadcast::{broadcast_spend_bundle, BroadcastSpendBundleResult};
pub use cats::{
    cat_from_parent_spend, child_cat_asset_ids_from_parent_spend, list_unspent_cats,
    list_unspent_cats_by_ids, require_cat_from_parent_spend,
};
pub use coin_select::{select_cats_smallest_first, SelectedCats, MIN_CAT_OUTPUT_MOJOS};
pub use direct_api::{
    effective_coinset_base_url, explicit_coinset_url_override, normalize_coinset_network,
    normalize_direct_base_url_input, resolve_coinset_endpoint, resolve_direct_client,
    resolve_direct_coinset_base_url, ResolvedCoinsetEndpoint, ResolvedDirectClient,
    DEFAULT_COINSET_BASE_URL, MAINNET_DIRECT_BASE_URL, TESTNET11_DIRECT_BASE_URL,
};
pub use json_util::{to_coinset_hex, u64_from_value};
pub use offer_assets::{lookup_asset_by_symbol, AssetInfo};
pub use parse::{
    coin_from_record, coin_id_from_record, coin_records_from_payload,
    coin_spend_from_solution_payload, record_from_payload,
};
pub use presplit::{fetch_offer_input_cat, wait_for_unspent_cat};
pub use probe::{build_coinset_probe_report, run_coinset_probe_command, CoinsetProbeCliArgs};
pub use retry::{
    with_coinset_client_retries, with_coinset_client_retries_with_policy, with_script_retries,
    with_script_retries_with_policy, ScriptRetryPolicy,
};
pub use rpc_result::ensure_coinset_rpc_success;
pub use scan_client::{DirectCoinsetScanClient, ResolvedDirectScanClient};
pub use signer_client::{client_for_network, client_for_signer_on_network};
pub use spent_verify::{wait_until_coins_spent, CoinSpentVerifyConfig};
pub use vault_fetch::fetch_latest_vault;
pub use wallet_io::{
    cat_outer_puzzle_hash_hex, extract_coin_id_hints_from_offer_text,
    list_wallet_unspent_coins_for_signer, puzzle_hash_hex_for_receive_address,
    spend_bundle_hash_from_hex, spend_bundle_hex, WalletUnspentCoin,
};
pub use xch::list_unspent_xch;

pub use chia_sdk_coinset::CoinsetClient;

use crate::error::SignerResult;

impl ResolvedCoinsetEndpoint {
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Coinset client for this resolved endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be constructed.
    pub fn client(&self) -> SignerResult<CoinsetClient> {
        direct_coinset_client(self.network, Some(self.base_url()))
    }
}

pub use crate::hex::parse_coin_ids;
