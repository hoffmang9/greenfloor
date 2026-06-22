use chia_protocol::Bytes32;
use chia_puzzle_types::Proof;
use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};
use chia_sdk_driver::{Vault, VaultInfo};
use clvm_utils::TreeHash;

use super::parse::coin_records_from_response;
use crate::error::{SignerError, SignerResult};

/// Fetch latest vault.
///
/// # Errors
///
/// Returns an error if the operation fails.
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
