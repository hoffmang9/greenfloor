use chia_protocol::{Bytes32, SpendBundle};
use chia_sdk_driver::{AssetInfo, Offer, SpendContext};

use crate::bech32m::{decode_offer, encode_offer};
use chia_sdk_types::{run_puzzle, Condition, Conditions};
use chia_traits::Streamable;
use clvm_traits::FromClvm;
use clvmr::{serde::node_from_bytes, Allocator, NodePtr};

use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;

type RequestedXchPayments = Vec<(Vec<u8>, Vec<(Vec<u8>, u64)>)>;
type RequestedCatPayments = Vec<(Vec<u8>, Vec<u8>, Vec<(Vec<u8>, u64)>)>;

fn parse_bytes32(raw: &[u8], field: &str) -> SignerResult<Bytes32> {
    let bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| SignerError::Other(format!("{field} must be 32 bytes")))?;
    Ok(Bytes32::new(bytes))
}

fn condition_has_offer_expiration(condition: &Condition<NodePtr>) -> bool {
    matches!(
        condition,
        Condition::AssertBeforeSecondsRelative(_)
            | Condition::AssertBeforeSecondsAbsolute(_)
            | Condition::AssertBeforeHeightRelative(_)
            | Condition::AssertBeforeHeightAbsolute(_)
    )
}

fn expires_at_seconds_from_condition(condition: &Condition<NodePtr>) -> Option<u64> {
    match condition {
        Condition::AssertBeforeSecondsAbsolute(seconds) => Some(seconds.seconds),
        Condition::AssertBeforeSecondsRelative(seconds) => Some(seconds.seconds),
        _ => None,
    }
}

fn parse_expires_at_seconds_from_coin_spend(
    coin_spend: &chia_protocol::CoinSpend,
) -> SignerResult<Option<u64>> {
    let mut allocator = Allocator::new();
    let puzzle = node_from_bytes(&mut allocator, coin_spend.puzzle_reveal.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let solution = node_from_bytes(&mut allocator, coin_spend.solution.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let output = run_puzzle(&mut allocator, puzzle, solution)
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let conditions = Conditions::<NodePtr>::from_clvm(&allocator, output)
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    for condition in conditions.iter() {
        if let Some(seconds) = expires_at_seconds_from_condition(condition) {
            return Ok(Some(seconds));
        }
    }
    Ok(None)
}

fn coin_spend_has_expiration_condition(
    coin_spend: &chia_protocol::CoinSpend,
) -> SignerResult<bool> {
    let mut allocator = Allocator::new();
    let puzzle = node_from_bytes(&mut allocator, coin_spend.puzzle_reveal.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let solution = node_from_bytes(&mut allocator, coin_spend.solution.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let output = run_puzzle(&mut allocator, puzzle, solution)
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let conditions = Conditions::<NodePtr>::from_clvm(&allocator, output)
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    for condition in conditions.iter() {
        if condition_has_offer_expiration(condition) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Extract offer expiry seconds from a coin spend's emitted conditions.
///
/// # Errors
///
/// Returns an error if puzzle execution fails.
pub fn expires_at_seconds_from_coin_spend(
    coin_spend: &chia_protocol::CoinSpend,
) -> SignerResult<Option<u64>> {
    parse_expires_at_seconds_from_coin_spend(coin_spend)
}

/// Extract offer expiry seconds from the offered coin's spend inside a bundle.
///
/// # Errors
///
/// Returns an error if the coin spend is missing or puzzle execution fails.
pub fn expires_at_seconds_from_offer_spend(
    spend_bundle: &SpendBundle,
    coin_id: Bytes32,
) -> SignerResult<Option<u64>> {
    let coin_spend = spend_bundle
        .coin_spends
        .iter()
        .find(|spend| spend.coin.coin_id() == coin_id)
        .ok_or(SignerError::OfferCancelNoSpendableInput)?;
    expires_at_seconds_from_coin_spend(coin_spend)
}

/// Offer has expiration condition.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn offer_has_expiration_condition(spend_bundle: &SpendBundle) -> SignerResult<bool> {
    for coin_spend in &spend_bundle.coin_spends {
        if coin_spend_has_expiration_condition(coin_spend)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[must_use]
pub fn offer_has_duplicate_spent_coin_ids(spend_bundle: &SpendBundle) -> bool {
    let mut seen = std::collections::HashSet::new();
    for coin_spend in &spend_bundle.coin_spends {
        let coin_id = coin_spend.coin.coin_id();
        let normalized = normalize_hex_id(&hex::encode(coin_id));
        if normalized.is_empty() {
            continue;
        }
        if !seen.insert(normalized) {
            return true;
        }
    }
    false
}

fn decode_and_parse_offer(offer: &str) -> SignerResult<SpendBundle> {
    let spend_bundle = decode_offer(offer)?;
    let mut ctx = SpendContext::new();
    Offer::from_spend_bundle(&mut ctx, &spend_bundle)?;
    Ok(spend_bundle)
}

/// Decode and parse offer structure (wallet-sdk semantics) without Dexie policy gates.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn validate_offer_structure(offer: &str) -> SignerResult<()> {
    decode_and_parse_offer(offer)?;
    Ok(())
}

/// Full Dexie pre-post validation: structure, expiry, and duplicate spends.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn validate_offer_text(offer: &str) -> SignerResult<()> {
    let spend_bundle = decode_and_parse_offer(offer)?;
    if offer_has_duplicate_spent_coin_ids(&spend_bundle) {
        return Err(SignerError::OfferDuplicateSpentCoinIds);
    }
    if !offer_has_expiration_condition(&spend_bundle)? {
        return Err(SignerError::OfferMissingExpiration);
    }
    Ok(())
}

fn dexie_verify_error_code(err: SignerError) -> String {
    match err {
        SignerError::OfferDuplicateSpentCoinIds => {
            "wallet_sdk_offer_duplicate_spent_coin_ids".to_string()
        }
        SignerError::OfferMissingExpiration => "wallet_sdk_offer_missing_expiration".to_string(),
        SignerError::Driver(msg) => format!("wallet_sdk_offer_validate_failed:driver:{msg}"),
        SignerError::Other(msg) => format!("wallet_sdk_offer_validate_failed:other:{msg}"),
        err => format!("wallet_sdk_offer_validate_failed:{err}"),
    }
}

/// Dexie pre-post gate returning a stable error code string, or ``None`` when valid.
#[must_use]
pub fn verify_offer_for_dexie(offer: &str) -> Option<String> {
    match validate_offer_text(offer) {
        Ok(()) => None,
        Err(err) => Some(dexie_verify_error_code(err)),
    }
}

/// Encode offer from spend bundle bytes.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn encode_offer_from_spend_bundle_bytes(spend_bundle_bytes: &[u8]) -> SignerResult<String> {
    let spend_bundle = SpendBundle::from_bytes(spend_bundle_bytes)
        .map_err(|err| SignerError::Other(format!("invalid_spend_bundle_bytes:{err}")))?;
    encode_offer(&spend_bundle)
}

/// From input spend bundle bytes.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn from_input_spend_bundle_bytes(
    spend_bundle_bytes: &[u8],
    requested_payments_xch: RequestedXchPayments,
    requested_payments_cats: RequestedCatPayments,
) -> SignerResult<Vec<u8>> {
    let spend_bundle = SpendBundle::from_bytes(spend_bundle_bytes)
        .map_err(|err| SignerError::Other(format!("invalid_spend_bundle_bytes:{err}")))?;

    let mut requested_payments = chia_sdk_driver::RequestedPayments::new();
    for (nonce_raw, payments_raw) in requested_payments_xch {
        let nonce = parse_bytes32(&nonce_raw, "nonce")?;
        let mut payments = Vec::with_capacity(payments_raw.len());
        for (puzzle_hash_raw, amount) in payments_raw {
            let puzzle_hash = parse_bytes32(&puzzle_hash_raw, "puzzle_hash")?;
            payments.push(chia_puzzle_types::offer::Payment::new(
                puzzle_hash,
                amount,
                chia_puzzle_types::Memos::None,
            ));
        }
        requested_payments
            .xch
            .push(chia_puzzle_types::offer::NotarizedPayment::new(
                nonce, payments,
            ));
    }
    for (asset_id_raw, nonce_raw, payments_raw) in requested_payments_cats {
        let asset_id = parse_bytes32(&asset_id_raw, "asset_id")?;
        let nonce = parse_bytes32(&nonce_raw, "nonce")?;
        let mut payments = Vec::with_capacity(payments_raw.len());
        for (puzzle_hash_raw, amount) in payments_raw {
            let puzzle_hash = parse_bytes32(&puzzle_hash_raw, "puzzle_hash")?;
            payments.push(chia_puzzle_types::offer::Payment::new(
                puzzle_hash,
                amount,
                chia_puzzle_types::Memos::None,
            ));
        }
        requested_payments.cats.entry(asset_id).or_default().push(
            chia_puzzle_types::offer::NotarizedPayment::new(nonce, payments),
        );
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

/// From input spend bundle xch bytes.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn from_input_spend_bundle_xch_bytes(
    spend_bundle_bytes: &[u8],
    requested_payments_xch: RequestedXchPayments,
) -> SignerResult<Vec<u8>> {
    from_input_spend_bundle_bytes(spend_bundle_bytes, requested_payments_xch, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::{
        encode_offer_from_spend_bundle_bytes, from_input_spend_bundle_xch_bytes,
        offer_has_duplicate_spent_coin_ids, verify_offer_for_dexie,
    };
    use chia_bls;
    use chia_protocol::{Coin, SpendBundle};
    use chia_traits::Streamable;

    #[test]
    fn duplicate_spent_coin_ids_detected() {
        let coin = Coin::new(
            chia_protocol::Bytes32::default(),
            chia_protocol::Bytes32::default(),
            1,
        );
        let spend = chia_protocol::CoinSpend::new(
            coin,
            chia_protocol::Program::default(),
            chia_protocol::Program::default(),
        );
        let bundle = SpendBundle::new(vec![spend.clone(), spend], chia_bls::Signature::default());
        assert!(offer_has_duplicate_spent_coin_ids(&bundle));
    }

    #[test]
    fn empty_bundle_has_no_duplicate_spends() {
        let bundle = SpendBundle::new(vec![], chia_bls::Signature::default());
        assert!(!offer_has_duplicate_spent_coin_ids(&bundle));
    }

    #[test]
    fn verify_offer_for_dexie_rejects_garbage_offer_text() {
        let error = verify_offer_for_dexie("not-an-offer").expect("error");
        assert!(error.contains("wallet_sdk_offer_validate_failed"));
    }

    #[test]
    fn encode_offer_from_spend_bundle_bytes_rejects_invalid_bytes() {
        let err = encode_offer_from_spend_bundle_bytes(b"not-a-bundle").unwrap_err();
        assert!(err.to_string().contains("invalid_spend_bundle_bytes"));
    }

    #[test]
    fn from_input_spend_bundle_xch_bytes_rejects_bad_nonce_length() {
        let bundle = SpendBundle::new(vec![], chia_bls::Signature::default());
        let bytes = bundle.to_bytes().expect("bytes");
        let err =
            from_input_spend_bundle_xch_bytes(&bytes, vec![(vec![0x01], vec![])]).unwrap_err();
        assert!(err.to_string().contains("nonce must be 32 bytes"));
    }
}
