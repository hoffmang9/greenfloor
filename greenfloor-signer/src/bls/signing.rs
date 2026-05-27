use chia_bls::{SecretKey, Signature, sign};
use chia_protocol::{Bytes32, CoinSpend};
use chia_sdk_signer::{AggSigConstants, RequiredSignature};
use chia_sdk_types::MAINNET_CONSTANTS;
use chia_sdk_types::TESTNET11_CONSTANTS;
use clvmr::Allocator;
use indexmap::IndexMap;

use crate::error::{SignerError, SignerResult};

pub fn sign_coin_spends(
    network: &str,
    coin_spends: &[CoinSpend],
    synthetic_sks: &IndexMap<Bytes32, SecretKey>,
) -> SignerResult<Signature> {
    let constants = agg_sig_constants(network)?;
    let mut allocator = Allocator::new();
    let required = RequiredSignature::from_coin_spends(&mut allocator, coin_spends, &constants)
        .map_err(|err| SignerError::Other(err.to_string()))?;
    let mut aggregate = Signature::default();
    for item in required {
        let chia_sdk_signer::RequiredSignature::Bls(required_bls) = item else {
            continue;
        };
        let pk = required_bls.public_key;
        let sk = synthetic_sks
            .values()
            .find(|sk| sk.public_key() == pk)
            .ok_or(SignerError::MissingSigningKeyForSelectedCoins)?;
        aggregate = chia_bls::aggregate(&[aggregate, sign(sk, required_bls.message())]);
    }
    Ok(aggregate)
}

fn agg_sig_constants(network: &str) -> SignerResult<AggSigConstants> {
    match network {
        "mainnet" => Ok(AggSigConstants::new(
            MAINNET_CONSTANTS.agg_sig_me_additional_data,
        )),
        "testnet11" => Ok(AggSigConstants::new(
            TESTNET11_CONSTANTS.agg_sig_me_additional_data,
        )),
        _ => Err(SignerError::UnsupportedNetworkForSigning),
    }
}
