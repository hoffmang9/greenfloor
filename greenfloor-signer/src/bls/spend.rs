use chia_bls::{PublicKey, SecretKey};
use chia_protocol::{Bytes32, Coin, SpendBundle};
use chia_sdk_driver::{Action, Cat, Relation, SpendContext, Spends};
use indexmap::IndexMap;
use std::collections::HashSet;

use crate::bls::keys::synthetic_secret_keys_for_puzzle_hashes;
use crate::bls::signing::sign_coin_spends;
use crate::error::SignerResult;

/// Synthetic BLS keys indexed by puzzle hash for offered coins.
pub struct SyntheticKeys {
    pub synthetic_sks: IndexMap<Bytes32, SecretKey>,
    pub synthetic_pks: IndexMap<Bytes32, PublicKey>,
}

pub fn puzzle_hashes_for_coins(xch_coins: &[Coin], cat_coins: &[Cat]) -> HashSet<Bytes32> {
    xch_coins
        .iter()
        .map(|coin| coin.puzzle_hash)
        .chain(cat_coins.iter().map(|cat| cat.info.p2_puzzle_hash))
        .collect()
}

pub fn synthetic_keys_for_puzzle_hashes(
    master_sk: &SecretKey,
    puzzle_hashes: &HashSet<Bytes32>,
) -> SignerResult<SyntheticKeys> {
    let synthetic_sks = synthetic_secret_keys_for_puzzle_hashes(master_sk, puzzle_hashes, None)?;
    let synthetic_pks: IndexMap<Bytes32, PublicKey> = synthetic_sks
        .iter()
        .map(|(puzzle_hash, sk)| (*puzzle_hash, sk.public_key()))
        .collect();
    Ok(SyntheticKeys {
        synthetic_sks,
        synthetic_pks,
    })
}

pub fn synthetic_keys_for_coins(
    master_sk: &SecretKey,
    xch_coins: &[Coin],
    cat_coins: &[Cat],
) -> SignerResult<SyntheticKeys> {
    synthetic_keys_for_puzzle_hashes(master_sk, &puzzle_hashes_for_coins(xch_coins, cat_coins))
}

pub fn add_coins_to_spends(
    spends: &mut Spends,
    xch_coins: impl IntoIterator<Item = Coin>,
    cat_coins: impl IntoIterator<Item = Cat>,
) {
    for coin in xch_coins {
        spends.add(coin);
    }
    for cat in cat_coins {
        spends.add(cat);
    }
}

/// Build and sign a spend bundle; *before_apply* may extend required conditions (e.g. offer assertions).
pub fn build_signed_spend(
    network: &str,
    receive_puzzle_hash: Bytes32,
    xch_coins: Vec<Coin>,
    cat_coins: Vec<Cat>,
    actions: Vec<Action>,
    master_sk: &SecretKey,
    before_apply: impl FnOnce(&mut Spends, &mut SpendContext) -> SignerResult<()>,
) -> SignerResult<(SpendBundle, SpendContext)> {
    let keys = synthetic_keys_for_coins(master_sk, &xch_coins, &cat_coins)?;
    let mut ctx = SpendContext::new();
    let mut spends = Spends::new(receive_puzzle_hash);
    add_coins_to_spends(&mut spends, xch_coins, cat_coins);
    before_apply(&mut spends, &mut ctx)?;
    let deltas = spends.apply(&mut ctx, &actions)?;
    spends.finish_with_keys(&mut ctx, &deltas, Relation::None, &keys.synthetic_pks)?;
    let coin_spends = ctx.take();
    let signature = sign_coin_spends(network, &coin_spends, &keys.synthetic_sks)?;
    Ok((SpendBundle::new(coin_spends, signature), ctx))
}

/// Standard wallet spend (XCH and/or CAT inputs, driver actions only).
pub fn build_signed_standard_spend(
    network: &str,
    receive_puzzle_hash: Bytes32,
    xch_coins: Vec<Coin>,
    cat_coins: Vec<Cat>,
    actions: Vec<Action>,
    master_sk: &SecretKey,
) -> SignerResult<SpendBundle> {
    build_signed_spend(
        network,
        receive_puzzle_hash,
        xch_coins,
        cat_coins,
        actions,
        master_sk,
        |_: &mut Spends, _: &mut SpendContext| Ok(()),
    )
    .map(|(spend_bundle, _ctx)| spend_bundle)
}
