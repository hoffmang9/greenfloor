use aws_sdk_kms::{primitives::Blob, Client};
use sha2::{Digest, Sha256};

use crate::error::{SignerError, SignerResult};

pub async fn get_public_key_compressed_hex(key_id: &str, region: &str) -> SignerResult<String> {
    let client = kms_client(region).await?;
    let response = client
        .get_public_key()
        .key_id(key_id)
        .send()
        .await
        .map_err(|err| SignerError::Kms(format!("GetPublicKey failed: {err}")))?;
    let der_bytes = response
        .public_key()
        .ok_or_else(|| SignerError::Kms("GetPublicKey returned no public key".to_string()))?;
    let compressed = der_spki_to_compressed_p256(der_bytes.as_ref())?;
    Ok(hex::encode(compressed))
}

pub async fn sign_digest(key_id: &str, region: &str, message_hex: &str) -> SignerResult<String> {
    let message_bytes = hex::decode(normalize_hex(message_hex))
        .map_err(|err| SignerError::Kms(format!("invalid message hex: {err}")))?;
    let digest = Sha256::digest(&message_bytes);
    let client = kms_client(region).await?;
    let response = client
        .sign()
        .key_id(key_id)
        .message(Blob::new(digest.to_vec()))
        .message_type(aws_sdk_kms::types::MessageType::Digest)
        .signing_algorithm(aws_sdk_kms::types::SigningAlgorithmSpec::EcdsaSha256)
        .send()
        .await
        .map_err(|err| SignerError::Kms(format!("Sign failed: {err}")))?;
    let der_sig = response
        .signature()
        .ok_or_else(|| SignerError::Kms("Sign returned no signature".to_string()))?;
    let compact = der_ecdsa_to_compact(der_sig.as_ref())?;
    Ok(hex::encode(compact))
}

async fn kms_client(region: &str) -> SignerResult<Client> {
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(region.to_string()))
        .load()
        .await;
    Ok(Client::new(&config))
}

pub fn der_spki_to_compressed_p256(der: &[u8]) -> SignerResult<[u8; 33]> {
    let (idx, _) = read_der_tag_length(der, 0)?;
    let (idx, algo_len) = read_der_tag_length(der, idx)?;
    let idx = idx + algo_len;
    if der.get(idx) != Some(&0x03) {
        return Err(SignerError::Kms(
            "expected BIT STRING tag (0x03)".to_string(),
        ));
    }
    let (idx, bs_len) = read_der_tag_length(der, idx)?;
    if der.get(idx) != Some(&0x00) {
        return Err(SignerError::Kms(format!(
            "unexpected unused-bits byte: {:#x}",
            der[idx]
        )));
    }
    let point = &der[idx + 1..idx + bs_len];
    if point.len() != 65 || point[0] != 0x04 {
        return Err(SignerError::Kms(format!(
            "expected 65-byte uncompressed point (0x04||x||y), got {} bytes",
            point.len()
        )));
    }
    let x = &point[1..33];
    let y = &point[33..65];
    let prefix = if y[y.len() - 1].is_multiple_of(2) {
        0x02
    } else {
        0x03
    };
    let mut compressed = [0u8; 33];
    compressed[0] = prefix;
    compressed[1..].copy_from_slice(x);
    Ok(compressed)
}

pub fn der_ecdsa_to_compact(der: &[u8]) -> SignerResult<[u8; 64]> {
    let (idx, _) = read_der_tag_length(der, 0)?;
    let (r, idx) = read_der_integer(der, idx)?;
    let (s, _) = read_der_integer(der, idx)?;
    let mut compact = [0u8; 64];
    compact[..32].copy_from_slice(&pad_to_32(&r));
    compact[32..].copy_from_slice(&pad_to_32(&s));
    Ok(compact)
}

fn pad_to_32(raw: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    if raw.len() <= 32 {
        out[32 - raw.len()..].copy_from_slice(raw);
    } else {
        out.copy_from_slice(&raw[raw.len() - 32..]);
    }
    out
}

fn read_der_tag_length(data: &[u8], offset: usize) -> SignerResult<(usize, usize)> {
    let offset = offset + 1;
    let first = *data
        .get(offset)
        .ok_or_else(|| SignerError::Kms("truncated DER".to_string()))?;
    if first & 0x80 == 0 {
        return Ok((offset + 1, first as usize));
    }
    let num_len_bytes = (first & 0x7F) as usize;
    let start = offset + 1;
    let end = start + num_len_bytes;
    let mut length = 0usize;
    for byte in &data[start..end] {
        length = (length << 8) | usize::from(*byte);
    }
    Ok((end, length))
}

fn read_der_integer(data: &[u8], offset: usize) -> SignerResult<(Vec<u8>, usize)> {
    if data.get(offset) != Some(&0x02) {
        return Err(SignerError::Kms(format!(
            "expected INTEGER tag (0x02), got {:#x}",
            data.get(offset).copied().unwrap_or_default()
        )));
    }
    let (offset, length) = read_der_tag_length(data, offset)?;
    let mut raw = data[offset..offset + length].to_vec();
    if raw.len() > 1 && raw[0] == 0x00 {
        raw.remove(0);
    }
    Ok((raw, offset + length))
}

pub fn normalize_hex(value: &str) -> String {
    let raw = value.trim().trim_start_matches("0x").to_ascii_lowercase();
    raw.chars().filter(char::is_ascii_hexdigit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_der_ecdsa_signature() {
        // SEQUENCE { INTEGER r, INTEGER s } with small values
        let der = [
            0x30, 0x44, 0x02, 0x20, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a,
            0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x02, 0x20, 0x21, 0x22, 0x23, 0x24,
            0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32,
            0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40,
        ];
        let compact = der_ecdsa_to_compact(&der).expect("parse der");
        assert_eq!(compact[31], 0x20);
        assert_eq!(compact[32], 0x21);
        assert_eq!(compact[63], 0x40);
    }
}
