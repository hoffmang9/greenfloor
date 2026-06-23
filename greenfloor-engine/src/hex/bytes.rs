use chia_protocol::Bytes32;

use crate::error::{SignerError, SignerResult};

use super::normalize_hex;

/// Hex to bytes.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn hex_to_bytes(value: &str) -> SignerResult<Vec<u8>> {
    let normalized = normalize_hex(value);
    if normalized.is_empty() || !normalized.len().is_multiple_of(2) {
        return Err(SignerError::Other(format!("invalid hex: {value}")));
    }
    hex::decode(normalized).map_err(|err| SignerError::Other(format!("invalid hex: {err}")))
}

/// Hex to bytes32.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn hex_to_bytes32(value: &str) -> SignerResult<Bytes32> {
    let bytes = hex_to_bytes(value)?;
    if bytes.len() != 32 {
        return Err(SignerError::Other(format!(
            "expected 32-byte hex value, got {} bytes",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(Bytes32::new(out))
}

/// Parse coin ids.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn parse_coin_ids(raw_values: &[String]) -> SignerResult<Vec<Bytes32>> {
    raw_values
        .iter()
        .map(|value| hex_to_bytes32(value))
        .collect()
}

/// Copy *bytes* into a fixed-size array when the length matches.
///
/// # Errors
///
/// Returns an error when `bytes.len()` does not equal `N`.
pub fn fixed_bytes<const N: usize>(bytes: &[u8]) -> SignerResult<[u8; N]> {
    if bytes.len() != N {
        return Err(SignerError::Other(format!(
            "expected {N}-byte value, got {} bytes",
            bytes.len()
        )));
    }
    let mut out = [0u8; N];
    out.copy_from_slice(bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{fixed_bytes, hex_to_bytes32};

    #[test]
    fn hex_to_bytes32_accepts_prefixed_input() {
        let raw = "ab".repeat(32);
        let bytes = hex_to_bytes32(&raw).expect("raw");
        let prefixed = hex_to_bytes32(&format!("0x{raw}")).expect("prefixed");
        assert_eq!(bytes, prefixed);
    }

    #[test]
    fn fixed_bytes_rejects_wrong_length() {
        assert!(fixed_bytes::<33>(&[0u8; 32]).is_err());
        assert_eq!(fixed_bytes::<33>(&[1u8; 33]).expect("33"), [1u8; 33]);
    }

    #[test]
    fn hex_to_bytes_and_parse_coin_ids_validate_input() {
        use super::{hex_to_bytes, parse_coin_ids};

        assert_eq!(hex_to_bytes("0x0102").expect("bytes"), vec![1, 2]);
        assert!(hex_to_bytes("0x0").is_err());
        let coin = "cd".repeat(32);
        let ids = parse_coin_ids(std::slice::from_ref(&coin)).expect("coin ids");
        assert_eq!(ids.len(), 1);
        assert_eq!(hex::encode(ids[0]), coin);
        assert!(parse_coin_ids(&["not-hex".to_string()]).is_err());
    }
}
