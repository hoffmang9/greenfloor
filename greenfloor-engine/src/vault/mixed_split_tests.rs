use super::{validate_mixed_split_request, MixedSplitRequest};
use crate::error::SignerError;
use chia_protocol::Bytes32;

fn sample_request(output_amounts: Vec<u64>, allow_sub_cat_output: bool) -> MixedSplitRequest {
    MixedSplitRequest {
        receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w"
            .to_string(),
        asset_id: Bytes32::default(),
        output_amounts,
        coin_ids: vec![Bytes32::default(), Bytes32::new([0xbb; 32])],
        allow_sub_cat_output,
        fee_mojos: 0,
    }
}

#[test]
fn rejects_sub_unit_cat_outputs() {
    let err = validate_mixed_split_request(&sample_request(vec![999], false)).unwrap_err();
    assert!(matches!(err, SignerError::CatOutputBelowMinimum));
}

#[test]
fn allow_sub_cat_output_bypasses_floor_guard() {
    validate_mixed_split_request(&sample_request(vec![999], true)).expect("allowed");
}

#[test]
fn rejects_vault_mixed_split_with_fee() {
    let mut request = sample_request(vec![1000], false);
    request.fee_mojos = 1;
    let err = validate_mixed_split_request(&request).unwrap_err();
    assert!(matches!(
        err,
        SignerError::MixedSplitVaultWithFeeNotSupported
    ));
}

#[test]
fn rejects_empty_output_amounts() {
    let err = validate_mixed_split_request(&sample_request(vec![], false)).unwrap_err();
    assert!(matches!(err, SignerError::MissingOutputAmounts));
}

#[test]
fn rejects_zero_output_amount() {
    let err = validate_mixed_split_request(&sample_request(vec![1000, 0], false)).unwrap_err();
    assert!(matches!(err, SignerError::InvalidOutputAmount));
}
