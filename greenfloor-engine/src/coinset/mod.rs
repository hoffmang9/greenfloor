mod api;
mod asset;
mod backend;
mod coin_select;
mod msp;
mod presplit;
mod wallet_io;
mod xch;

pub use api::{
    conservative_fee_from_payload, get_all_mempool_tx_ids, get_conservative_fee_estimate,
    get_fee_estimate, post_coinset_rpc, push_tx_hex,
};

pub(crate) use coin_select::finalize_selected_cats;

pub use asset::is_canonical_xch_asset;
pub use asset::is_xch_like_asset;

pub use backend::{LiveCoinset, OfferCoinsetBackend};
pub use msp::{
    normalize_asset_id, resolve_offer_asset_ids, AssetInfo, MspCoinset, SingletonInfo,
    DEFAULT_MSP_BASE_URL,
};
pub use presplit::{fetch_presplit_cat_by_id, wait_for_unspent_cat};
pub use wallet_io::{
    cat_outer_puzzle_hash_hex, extract_coin_id_hints_from_offer_text, list_wallet_unspent_coins,
    puzzle_hash_hex_for_receive_address, spend_bundle_hash_from_hex, WalletUnspentCoin,
};

use chia_protocol::{Bytes32, Coin, CoinSpend, SpendBundle};
use chia_puzzle_types::cat::CatArgs;
use chia_puzzle_types::Proof;
use chia_sdk_coinset::{
    ChiaRpcClient, CoinRecord, GetCoinRecordResponse, GetCoinRecordsResponse,
    GetPuzzleAndSolutionResponse,
};

pub use chia_sdk_coinset::CoinsetClient;
use chia_sdk_driver::{Cat, Puzzle, Vault, VaultInfo};
use chia_sdk_utils::Address;
use chia_traits::Streamable;
use clvm_utils::TreeHash;
use clvmr::{serde::node_from_bytes, Allocator};

use crate::error::{SignerError, SignerResult};
use crate::vault::members::hex_to_bytes32;

pub const MIN_CAT_OUTPUT_MOJOS: u64 = 1000;

pub fn client_for_network(network: &str) -> SignerResult<CoinsetClient> {
    MspCoinset::for_network(network, None).map(|msp| msp.client().clone())
}

pub fn client_for_config(config: &crate::config::SignerConfig) -> SignerResult<CoinsetClient> {
    Ok(MspCoinset::new(&config.coinset_msp_base_url)
        .client()
        .clone())
}

pub fn decode_receive_address(receive_address: &str) -> SignerResult<Bytes32> {
    Address::decode(receive_address)
        .map_err(|err| SignerError::Other(format!("invalid receive address: {err}")))
        .map(|address| address.puzzle_hash)
}

#[derive(Debug, Clone)]
pub struct SelectedCats {
    pub selected: Vec<Cat>,
    pub offered_total: u64,
    pub change_amount: u64,
}

pub(crate) async fn select_cats_for_spend(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
) -> SignerResult<SelectedCats> {
    let cats = if explicit_coin_ids.is_empty() {
        list_unspent_cats(client, receive_address, asset_id).await?
    } else {
        list_unspent_cats_by_ids(client, explicit_coin_ids).await?
    };
    finalize_selected_cats(cats, explicit_coin_ids, target_amount)
}

pub async fn list_unspent_xch(
    client: &CoinsetClient,
    receive_address: &str,
) -> SignerResult<Vec<Coin>> {
    xch::list_unspent_xch(client, receive_address).await
}

pub async fn list_unspent_cats(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
) -> SignerResult<Vec<Cat>> {
    let puzzle_hash = decode_receive_address(receive_address)?;
    let cat_outer_puzzle_hash = CatArgs::curry_tree_hash(asset_id, puzzle_hash.into()).into();
    let response = client
        .get_coin_records_by_puzzle_hash(cat_outer_puzzle_hash, None, None, Some(false), None)
        .await
        .map_err(SignerError::from)?;
    let records = coin_records_from_response(response)?;
    let mut cats = Vec::new();
    for record in records {
        if record.spent {
            continue;
        }
        if let Some(cat) = cat_from_record(client, &record).await? {
            cats.push(cat);
        }
    }
    Ok(cats)
}

pub async fn list_unspent_cats_by_ids(
    client: &CoinsetClient,
    coin_ids: &[Bytes32],
) -> SignerResult<Vec<Cat>> {
    let mut cats = Vec::new();
    for coin_id in coin_ids {
        let response = client
            .get_coin_record_by_name(*coin_id)
            .await
            .map_err(SignerError::from)?;
        let Some(record) = response.coin_record else {
            continue;
        };
        if record.spent {
            continue;
        }
        if let Some(cat) = cat_from_record(client, &record).await? {
            cats.push(cat);
        }
    }
    Ok(cats)
}

pub async fn fetch_latest_vault(
    client: &CoinsetClient,
    launcher_id: Bytes32,
    inner_puzzle_hash: TreeHash,
) -> SignerResult<Vault> {
    let response = client
        .get_coin_records_by_parent_ids(vec![launcher_id], None, None, Some(true), None)
        .await
        .map_err(SignerError::from)?;
    let launcher_children = coin_records_from_response(response)?;
    let Some(first_child) = launcher_children.first() else {
        return Err(SignerError::VaultSingletonNotFound);
    };
    let singleton_puzzle_hash = first_child.coin.puzzle_hash;
    let leaf_response = client
        .get_coin_records_by_puzzle_hash(singleton_puzzle_hash, None, None, Some(false), None)
        .await
        .map_err(SignerError::from)?;
    let mut leaf_candidates = coin_records_from_response(leaf_response)?;
    if leaf_candidates.is_empty() {
        return Err(SignerError::VaultSingletonNotFound);
    }
    leaf_candidates.sort_by_key(|record| std::cmp::Reverse(record.confirmed_block_index));
    let current = &leaf_candidates[0];
    let parent_id = current.coin.parent_coin_info;
    let parent_response = client
        .get_coin_record_by_name(parent_id)
        .await
        .map_err(SignerError::from)?;
    let Some(parent_record) = parent_response.coin_record else {
        return Err(SignerError::VaultSingletonNotFound);
    };
    let parent_parent = parent_record.coin.parent_coin_info;
    let proof = if parent_id == launcher_id {
        Proof::Eve(chia_puzzle_types::EveProof {
            parent_parent_coin_info: parent_parent,
            parent_amount: parent_record.coin.amount,
        })
    } else {
        Proof::Lineage(chia_puzzle_types::LineageProof {
            parent_parent_coin_info: parent_parent,
            parent_inner_puzzle_hash: inner_puzzle_hash.into(),
            parent_amount: parent_record.coin.amount,
        })
    };
    Ok(Vault::new(
        current.coin,
        proof,
        VaultInfo::new(launcher_id, inner_puzzle_hash),
    ))
}

#[derive(Debug, Clone)]
pub struct BroadcastSpendBundleResult {
    pub status: String,
    pub operation_id: String,
}

pub async fn broadcast_spend_bundle(
    client: &CoinsetClient,
    spend_bundle: SpendBundle,
) -> SignerResult<BroadcastSpendBundleResult> {
    let operation_id = format!("0x{}", hex::encode(spend_bundle.hash()));
    // Coinset RPC expects structured SpendBundle JSON (not a hex string).
    let response = client
        .push_tx(spend_bundle)
        .await
        .map_err(SignerError::from)?;
    if !response.success {
        return Err(SignerError::Coinset(
            response
                .error
                .unwrap_or_else(|| "push_tx failed".to_string()),
        ));
    }
    Ok(BroadcastSpendBundleResult {
        status: response.status,
        operation_id,
    })
}

pub fn select_cats_smallest_first(cats: Vec<Cat>, target_total: u64) -> Vec<Cat> {
    let mut sorted = cats;
    sorted.sort_by_key(|cat| cat.coin.amount);
    let mut selected = Vec::new();
    let mut running = 0u64;
    for cat in sorted {
        running = running.saturating_add(cat.coin.amount);
        selected.push(cat);
        if running >= target_total {
            return selected;
        }
    }
    Vec::new()
}

pub(crate) async fn cat_from_record(
    client: &CoinsetClient,
    record: &CoinRecord,
) -> SignerResult<Option<Cat>> {
    let parent_response: GetCoinRecordResponse = client
        .get_coin_record_by_name(record.coin.parent_coin_info)
        .await
        .map_err(SignerError::from)?;
    let Some(parent_record) = parent_response.coin_record else {
        return Ok(None);
    };
    if parent_record.spent_block_index == 0 {
        return Ok(None);
    }
    let solution_response: GetPuzzleAndSolutionResponse = client
        .get_puzzle_and_solution(
            parent_record.coin.coin_id(),
            Some(parent_record.spent_block_index),
        )
        .await
        .map_err(SignerError::from)?;
    let Some(parent_spend) = solution_response.coin_solution else {
        return Ok(None);
    };
    parse_cat_from_parent_spend(record.coin, &parent_spend)
}

pub fn cat_from_parent_spend(coin: Coin, parent_spend: &CoinSpend) -> SignerResult<Option<Cat>> {
    parse_cat_from_parent_spend(coin, parent_spend)
}

pub fn require_cat_from_parent_spend(coin: Coin, parent_spend: &CoinSpend) -> SignerResult<Cat> {
    cat_from_parent_spend(coin, parent_spend)?.ok_or(SignerError::PresplitCoinNotFound)
}

fn parse_cat_from_parent_spend(coin: Coin, parent_spend: &CoinSpend) -> SignerResult<Option<Cat>> {
    let mut allocator = Allocator::new();
    let parent_puzzle_ptr = node_from_bytes(&mut allocator, parent_spend.puzzle_reveal.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let parent_solution_ptr = node_from_bytes(&mut allocator, parent_spend.solution.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let parent_puzzle = Puzzle::parse(&allocator, parent_puzzle_ptr);
    let children = Cat::parse_children(
        &mut allocator,
        parent_spend.coin,
        parent_puzzle,
        parent_solution_ptr,
    )
    .map_err(|err| SignerError::Driver(err.to_string()))?;
    let Some(children) = children else {
        return Ok(None);
    };
    Ok(children
        .into_iter()
        .find(|cat| cat.coin.coin_id() == coin.coin_id()))
}

fn coin_records_from_response(response: GetCoinRecordsResponse) -> SignerResult<Vec<CoinRecord>> {
    if !response.success {
        return Err(SignerError::Coinset(
            response
                .error
                .unwrap_or_else(|| "coinset request failed".to_string()),
        ));
    }
    Ok(response.coin_records.unwrap_or_default())
}

pub fn parse_coin_ids(raw_values: &[String]) -> SignerResult<Vec<Bytes32>> {
    raw_values
        .iter()
        .map(|value| hex_to_bytes32(value))
        .collect()
}

pub fn spend_bundle_hex(spend_bundle: &SpendBundle) -> SignerResult<String> {
    Ok(hex::encode(spend_bundle.to_bytes().map_err(|err| {
        SignerError::Other(format!("failed to serialize spend bundle: {err}"))
    })?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_cats_smallest_first_accumulates_until_target() {
        use chia_protocol::{Bytes32, Coin};
        use chia_sdk_driver::{Cat, CatInfo};

        fn cat_with_amount(amount: u64) -> Cat {
            Cat::new(
                Coin::new(
                    Bytes32::new([u8::try_from(amount).unwrap_or(0u8); 32]),
                    Bytes32::default(),
                    amount,
                ),
                None,
                CatInfo::new(Bytes32::new([0x01; 32]), None, Bytes32::default()),
            )
        }

        let cats = vec![
            cat_with_amount(5000),
            cat_with_amount(1000),
            cat_with_amount(3000),
        ];
        let selected = select_cats_smallest_first(cats, 2500);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].coin.amount, 1000);
        assert_eq!(selected[1].coin.amount, 3000);
    }
}
