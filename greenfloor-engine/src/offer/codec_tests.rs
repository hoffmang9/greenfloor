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
    let err = from_input_spend_bundle_xch_bytes(&bytes, vec![(vec![0x01], vec![])]).unwrap_err();
    assert!(err.to_string().contains("nonce must be 32 bytes"));
}
