use chia_protocol::{Bytes32, SpendBundle};
use chia_puzzle_types::{
    offer::{NotarizedPayment, Payment},
    Memos,
};
use chia_sdk_driver::{
    decode_offer, encode_offer as driver_encode_offer, AssetInfo, Offer, RequestedPayments,
    SpendContext,
};
use chia_traits::Streamable;

use crate::error::{SignerError, SignerResult};

fn parse_bytes32(raw: &[u8], field: &str) -> SignerResult<Bytes32> {
    let bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| SignerError::Other(format!("{field} must be 32 bytes")))?;
    Ok(Bytes32::new(bytes))
}

pub fn validate_offer_text(offer: &str) -> SignerResult<()> {
    let spend_bundle = decode_offer(offer)?;
    let mut ctx = SpendContext::new();
    Offer::from_spend_bundle(&mut ctx, &spend_bundle)?;
    Ok(())
}

pub fn encode_offer_from_spend_bundle_bytes(spend_bundle_bytes: &[u8]) -> SignerResult<String> {
    let spend_bundle = SpendBundle::from_bytes(spend_bundle_bytes)
        .map_err(|err| SignerError::Other(format!("invalid_spend_bundle_bytes:{err}")))?;
    driver_encode_offer(&spend_bundle).map_err(SignerError::from)
}

pub fn from_input_spend_bundle_bytes(
    spend_bundle_bytes: &[u8],
    requested_payments_xch: Vec<(Vec<u8>, Vec<(Vec<u8>, u64)>)>,
    requested_payments_cats: Vec<(Vec<u8>, Vec<u8>, Vec<(Vec<u8>, u64)>)>,
) -> SignerResult<Vec<u8>> {
    let spend_bundle = SpendBundle::from_bytes(spend_bundle_bytes)
        .map_err(|err| SignerError::Other(format!("invalid_spend_bundle_bytes:{err}")))?;

    let mut requested_payments = RequestedPayments::new();
    for (nonce_raw, payments_raw) in requested_payments_xch {
        let nonce = parse_bytes32(&nonce_raw, "nonce")?;
        let mut payments = Vec::with_capacity(payments_raw.len());
        for (puzzle_hash_raw, amount) in payments_raw {
            let puzzle_hash = parse_bytes32(&puzzle_hash_raw, "puzzle_hash")?;
            payments.push(Payment::new(puzzle_hash, amount, Memos::None));
        }
        requested_payments
            .xch
            .push(NotarizedPayment::new(nonce, payments));
    }
    for (asset_id_raw, nonce_raw, payments_raw) in requested_payments_cats {
        let asset_id = parse_bytes32(&asset_id_raw, "asset_id")?;
        let nonce = parse_bytes32(&nonce_raw, "nonce")?;
        let mut payments = Vec::with_capacity(payments_raw.len());
        for (puzzle_hash_raw, amount) in payments_raw {
            let puzzle_hash = parse_bytes32(&puzzle_hash_raw, "puzzle_hash")?;
            payments.push(Payment::new(puzzle_hash, amount, Memos::None));
        }
        requested_payments
            .cats
            .entry(asset_id)
            .or_default()
            .push(NotarizedPayment::new(nonce, payments));
    }

    let mut ctx = SpendContext::new();
    let offer = Offer::from_input_spend_bundle(
        &mut ctx,
        spend_bundle,
        requested_payments,
        AssetInfo::new(),
    )?;
    let offer_spend_bundle = offer.to_spend_bundle(&mut ctx)?;
    offer_spend_bundle
        .to_bytes()
        .map_err(|err| SignerError::Other(format!("offer_spend_bundle_to_bytes:{err}")))
}

pub fn from_input_spend_bundle_xch_bytes(
    spend_bundle_bytes: &[u8],
    requested_payments_xch: Vec<(Vec<u8>, Vec<(Vec<u8>, u64)>)>,
) -> SignerResult<Vec<u8>> {
    from_input_spend_bundle_bytes(spend_bundle_bytes, requested_payments_xch, Vec::new())
}
