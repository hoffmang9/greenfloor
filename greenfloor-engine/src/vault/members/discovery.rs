use chia_protocol::Bytes32;
use clvm_utils::TreeHash;

use crate::error::SignerResult;
use crate::hex::{hex_to_bytes32, tree_hash_to_hex};

use super::config::MemberConfig;
use super::curves::singleton_member_hash;

fn nonce_member_config(nonce: u32) -> MemberConfig {
    MemberConfig::default()
        .with_top_level(true)
        .with_nonce(nonce)
}

/// Top-level nonce member puzzle hash for vault singleton P2 discovery (no fast-forward).
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn nonce_member_puzzle_hash(launcher_id: Bytes32, nonce: u32) -> SignerResult<TreeHash> {
    singleton_member_hash(&nonce_member_config(nonce), launcher_id, false)
}

/// Hex-encoded nonce member puzzle hash for vault singleton P2 discovery.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn nonce_member_puzzle_hash_hex(launcher_id: Bytes32, nonce: u32) -> SignerResult<String> {
    Ok(tree_hash_to_hex(nonce_member_puzzle_hash(
        launcher_id,
        nonce,
    )?))
}

/// Hex-encoded nonce member puzzle hash from a normalized launcher id string.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn nonce_member_puzzle_hash_hex_from_launcher_id(
    launcher_id: &str,
    nonce: u32,
) -> SignerResult<String> {
    nonce_member_puzzle_hash_hex(hex_to_bytes32(launcher_id)?, nonce)
}

#[cfg(test)]
mod tests {
    use chia_protocol::Bytes32;

    use super::{
        nonce_member_puzzle_hash, nonce_member_puzzle_hash_hex,
        nonce_member_puzzle_hash_hex_from_launcher_id,
    };
    use crate::hex::{hex_to_bytes32, tree_hash_to_hex};

    #[test]
    fn nonce_member_puzzle_hash_hex_matches_tree_hash_hex() {
        let launcher = Bytes32::new([0x44; 32]);
        let hash = nonce_member_puzzle_hash(launcher, 3).expect("hash");
        let hex = nonce_member_puzzle_hash_hex(launcher, 3).expect("hex");
        assert_eq!(hex.len(), 64);
        assert_eq!(hex, tree_hash_to_hex(hash));
    }

    #[test]
    fn nonce_member_puzzle_hash_hex_from_launcher_id_normalizes_input() {
        let launcher = "ab".repeat(32);
        let from_string = nonce_member_puzzle_hash_hex_from_launcher_id(&launcher, 0).expect("hex");
        let launcher_bytes = hex_to_bytes32(&launcher).expect("launcher bytes");
        let direct = nonce_member_puzzle_hash_hex(launcher_bytes, 0).expect("direct");
        assert_eq!(from_string, direct);

        let from_prefixed =
            nonce_member_puzzle_hash_hex_from_launcher_id(&format!("0x{launcher}"), 0)
                .expect("prefixed");
        assert_eq!(from_string, from_prefixed);
    }

    #[test]
    fn nonce_member_puzzle_hash_changes_with_nonce() {
        let launcher = Bytes32::new([0x55; 32]);
        let nonce0 = nonce_member_puzzle_hash_hex(launcher, 0).expect("nonce0");
        let nonce1 = nonce_member_puzzle_hash_hex(launcher, 1).expect("nonce1");
        assert_ne!(nonce0, nonce1);
    }
}
