use chia_protocol::Bytes32;
use chia_sdk_utils::Address;

use crate::error::{SignerError, SignerResult};

/// Decode receive address.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn decode_receive_address(receive_address: &str) -> SignerResult<Bytes32> {
    Address::decode(receive_address)
        .map_err(|err| SignerError::Other(format!("invalid receive address: {err}")))
        .map(|address| address.puzzle_hash)
}
