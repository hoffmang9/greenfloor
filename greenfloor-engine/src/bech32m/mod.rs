//! Bech32m encode/decode for Chia addresses and offer files.
//!
//! This module is the **only** place in `greenfloor-engine` that performs Bech32m
//! encoding or decoding. Dexie and Coinset are transport layers: they may carry
//! already-encoded strings (offer files, addresses in config), but never decode them.

use chia_protocol::{Bytes32, SpendBundle};
use chia_sdk_utils::Address;

use crate::error::{SignerError, SignerResult};

/// Decode a Chia receive address (`xch1…`, `txch1…`, …) to a puzzle hash.
///
/// # Errors
///
/// Returns an error if the address is not valid Bech32m.
pub fn decode_address(address: &str) -> SignerResult<Bytes32> {
    Address::decode(address)
        .map_err(|err| SignerError::Other(format!("invalid receive address: {err}")))
        .map(|decoded| decoded.puzzle_hash)
}

/// Encode a puzzle hash as a Chia receive address with the given HRP prefix.
///
/// # Errors
///
/// Returns an error if encoding fails.
pub fn encode_address(puzzle_hash: Bytes32, prefix: &str) -> SignerResult<String> {
    Address::new(puzzle_hash, prefix.to_string())
        .encode()
        .map_err(|err| SignerError::Other(format!("invalid address encode: {err}")))
}

/// Decode an offer file string (`offer1…`) to a spend bundle.
///
/// # Errors
///
/// Returns an error if the offer text is not valid Bech32m or spend bundle bytes.
pub fn decode_offer(offer: &str) -> SignerResult<SpendBundle> {
    chia_sdk_driver::decode_offer(offer).map_err(SignerError::from)
}

/// Encode a spend bundle as an offer file string (`offer1…`).
///
/// # Errors
///
/// Returns an error if encoding fails.
pub fn encode_offer(spend_bundle: &SpendBundle) -> SignerResult<String> {
    chia_sdk_driver::encode_offer(spend_bundle).map_err(SignerError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_address_round_trips_known_mainnet_address() {
        let expected = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
        let puzzle_hash = decode_address(expected).expect("decode");
        let encoded = encode_address(puzzle_hash, "xch").expect("encode");
        assert_eq!(encoded, expected);
    }

    #[test]
    fn decode_address_rejects_bech32_not_bech32m() {
        let err = decode_address("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq").unwrap_err();
        assert!(err.to_string().contains("invalid receive address"));
    }
}
