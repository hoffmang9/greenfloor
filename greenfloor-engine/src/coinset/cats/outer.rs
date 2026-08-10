//! Canonical CAT outer puzzle-hash primitive.
//!
//! All `GreenFloor` `cat(asset_id, p2)` outer hashes go through [`cat_outer_puzzle_hash`].
//! Coinset hex is a thin format adapter; vault double-wrap assert policy stays in
//! `vault::cat_create`. Callers that need bare hex compare via
//! [`crate::hex::normalize_hex_id`].

use chia_protocol::Bytes32;
use chia_puzzle_types::cat::CatArgs;

use crate::coinset::to_coinset_hex;
use crate::hex::hex_to_bytes32;

/// `cat(asset_id, p2)` outer puzzle hash.
#[must_use]
pub(crate) fn cat_outer_puzzle_hash(asset_id: Bytes32, p2_puzzle_hash: Bytes32) -> Bytes32 {
    CatArgs::curry_tree_hash(asset_id, p2_puzzle_hash.into()).into()
}

/// Outer puzzle hash for Coinset puzzle-hash queries (`0x…`).
#[must_use]
pub(crate) fn cat_outer_coinset_hex(asset_id_hex: &str, p2_hex: &str) -> Option<String> {
    let asset = hex_to_bytes32(asset_id_hex).ok()?;
    let p2 = hex_to_bytes32(p2_hex).ok()?;
    Some(to_coinset_hex(cat_outer_puzzle_hash(asset, p2).as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::normalize_hex_id;

    #[test]
    fn coinset_hex_normalizes_to_bare_id() {
        let asset = "aa".repeat(32);
        let p2 = "11".repeat(32);
        let coinset = cat_outer_coinset_hex(&asset, &p2).expect("coinset");
        let normalized = normalize_hex_id(&coinset);
        assert_eq!(normalized.len(), 64);
        assert_eq!(
            cat_outer_puzzle_hash(
                hex_to_bytes32(&asset).expect("asset"),
                hex_to_bytes32(&p2).expect("p2"),
            ),
            hex_to_bytes32(&normalized).expect("round-trip")
        );
    }
}
