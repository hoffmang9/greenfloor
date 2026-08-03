//! CAT outer puzzle-hash helpers shared by discovery and prelabel.

use chia_protocol::Bytes32;
use chia_puzzle_types::cat::CatArgs;

use crate::coinset::to_coinset_hex;
use crate::hex::{hex_to_bytes32, normalize_hex_id};

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

/// Normalized outer puzzle hash for comparing against scanned coin rows.
#[must_use]
pub(crate) fn cat_outer_normalized_hex(asset_id_hex: &str, p2_hex: &str) -> Option<String> {
    let asset = hex_to_bytes32(asset_id_hex).ok()?;
    let p2 = hex_to_bytes32(p2_hex).ok()?;
    Some(normalize_hex_id(&hex::encode(cat_outer_puzzle_hash(
        asset, p2,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outer_helpers_agree_on_payload() {
        let asset = "aa".repeat(32);
        let p2 = "11".repeat(32);
        let coinset = cat_outer_coinset_hex(&asset, &p2).expect("coinset");
        let normalized = cat_outer_normalized_hex(&asset, &p2).expect("normalized");
        assert_eq!(normalize_hex_id(&coinset), normalized);
    }
}
