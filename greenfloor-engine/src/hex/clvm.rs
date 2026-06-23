use chia_protocol::Bytes32;
use clvm_utils::{tree_hash_atom, TreeHash};

#[must_use]
pub fn tree_hash_nil() -> TreeHash {
    tree_hash_atom(&[])
}

#[must_use]
pub fn tree_hash_to_hex(hash: TreeHash) -> String {
    hex::encode(hash.to_bytes())
}

/// Parse a 32-byte tree hash from lowercase hex.
///
/// # Errors
///
/// Returns an error when the value is not valid 32-byte hex.
pub fn hex_to_tree_hash(value: &str) -> crate::error::SignerResult<TreeHash> {
    let bytes = crate::hex::hex_to_bytes32(value)?;
    Ok(TreeHash::new(bytes.into()))
}

#[must_use]
pub fn bytes32_to_hex(value: Bytes32) -> String {
    hex::encode(value.to_bytes())
}

#[cfg(test)]
mod tests {
    use chia_protocol::Bytes32;

    use super::bytes32_to_hex;
    use crate::hex::hex_to_bytes32;

    #[test]
    fn bytes32_hex_round_trip_via_hex_to_bytes32() {
        let launcher = Bytes32::new([0x44; 32]);
        let parsed = hex_to_bytes32(&bytes32_to_hex(launcher)).expect("parse");
        assert_eq!(launcher, parsed);
    }
}
