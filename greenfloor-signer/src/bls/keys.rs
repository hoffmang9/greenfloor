use std::collections::HashSet;

use chia_bls::{DerivableKey, SecretKey};
use chia_protocol::Bytes32;
use chia_puzzle_types::standard::StandardArgs;
use chia_puzzle_types::DeriveSynthetic;
use indexmap::IndexMap;

use crate::error::{SignerError, SignerResult};

const WALLET_PATH_PREFIX: [u32; 3] = [12381, 8444, 2];
const DEFAULT_DERIVATION_SCAN_LIMIT: u32 = 200;

fn puzzle_hash_for_synthetic_secret_key(secret_key: &SecretKey) -> Bytes32 {
    StandardArgs::curry_tree_hash(secret_key.public_key()).into()
}

fn derive_wallet_child(master_sk: &SecretKey, index: u32, hardened: bool) -> SecretKey {
    let mut result = master_sk.clone();
    for &path_index in WALLET_PATH_PREFIX.iter().chain(std::iter::once(&index)) {
        result = if hardened {
            result.derive_hardened(path_index)
        } else {
            result.derive_unhardened(path_index)
        };
    }
    result
}

pub fn synthetic_secret_keys_for_puzzle_hashes(
    master_sk: &SecretKey,
    required_puzzle_hashes: &HashSet<Bytes32>,
    scan_limit: Option<u32>,
) -> SignerResult<IndexMap<Bytes32, SecretKey>> {
    if required_puzzle_hashes.is_empty() {
        return Ok(IndexMap::new());
    }
    let limit = scan_limit.unwrap_or(DEFAULT_DERIVATION_SCAN_LIMIT);
    let mut found = IndexMap::new();
    for index in 0..limit {
        for hardened in [false, true] {
            let child = derive_wallet_child(master_sk, index, hardened);
            let synthetic = child.derive_synthetic();
            let puzzle_hash = puzzle_hash_for_synthetic_secret_key(&synthetic);
            if required_puzzle_hashes.contains(&puzzle_hash) && !found.contains_key(&puzzle_hash) {
                found.insert(puzzle_hash, synthetic);
            }
        }
        if found.len() == required_puzzle_hashes.len() {
            return Ok(found);
        }
    }
    Err(SignerError::MissingSigningKeyForSelectedCoins)
}
