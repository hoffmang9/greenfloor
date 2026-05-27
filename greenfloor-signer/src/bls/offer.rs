use std::collections::HashSet;

use chia_bls::SecretKey;
use chia_protocol::{Bytes32, Coin, SpendBundle};
use chia_puzzle_types::Memos;
use chia_puzzles::SETTLEMENT_PAYMENT_HASH;
use chia_sdk_driver::{Action, AssetInfo, Cat, Id, Offer, Relation, SpendContext, Spends};
use chia_sdk_utils::select_coins;
use clvmr::Allocator;
use serde::{Deserialize, Serialize};

use crate::bls::coins::cat_asset_bytes;
use crate::bls::spend::{add_coins_to_spends, synthetic_keys_for_coins};
use crate::bls::signing::sign_coin_spends;
use crate::coinset::is_xch_like_asset;
use crate::coinset::{
    client_for_network, list_unspent_cats, list_unspent_cats_by_ids, list_unspent_xch,
    select_cats_smallest_first,
};
use crate::error::{SignerError, SignerResult};
use crate::offer::plan::build_requested_payments;
use crate::offer::types::OfferTerms;

#[derive(Debug, Clone, Deserialize)]
pub struct BlsOfferRequest {
    pub receive_address: String,
    pub offer_asset_id: String,
    pub offer_amount: u64,
    pub request_asset_id: String,
    pub request_amount: u64,
    #[serde(default)]
    pub offer_coin_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlsOfferResult {
    pub spend_bundle_hex: String,
}

pub async fn build_bls_offer_spend_bundle(
    network: &str,
    master_sk: &SecretKey,
    request: BlsOfferRequest,
) -> SignerResult<BlsOfferResult> {
    if request.offer_amount == 0 || request.request_amount == 0 {
        return Err(SignerError::InvalidOutputAmount);
    }

    let client = client_for_network(network)?;
    let receive_address = request.receive_address.trim();
    let receive_puzzle_hash = crate::coinset::decode_receive_address(receive_address)?;
    let offer_asset_raw = request.offer_asset_id.trim().to_lowercase();
    let explicit_coin_ids = crate::coinset::parse_coin_ids(&request.offer_coin_ids)?;

    let mut offered_xch: Vec<Coin> = Vec::new();
    let mut offered_cats: Vec<Cat> = Vec::new();

    if is_xch_like_asset(&offer_asset_raw) {
        let mut xch_coins = list_unspent_xch(&client, receive_address).await?;
        if !explicit_coin_ids.is_empty() {
            let allowed: HashSet<Bytes32> = explicit_coin_ids.iter().copied().collect();
            xch_coins.retain(|coin| allowed.contains(&coin.coin_id()));
        }
        if xch_coins.is_empty() {
            return Err(SignerError::NoUnspentOfferXchCoins);
        }
        offered_xch = select_coins(xch_coins, request.offer_amount)
            .map_err(|_| SignerError::InsufficientOfferXchCoins)?;
    } else {
        let asset_bytes = cat_asset_bytes(&offer_asset_raw)?;
        let cats = if explicit_coin_ids.is_empty() {
            list_unspent_cats(&client, receive_address, asset_bytes).await?
        } else {
            list_unspent_cats_by_ids(&client, &explicit_coin_ids).await?
        };
        if cats.is_empty() {
            return Err(SignerError::NoUnspentOfferCatCoins);
        }
        offered_cats = select_cats_smallest_first(cats, request.offer_amount);
        if offered_cats.is_empty() {
            return Err(SignerError::InsufficientOfferCatCoins);
        }
    }

    let offered_total: u64 = offered_xch.iter().map(|c| c.amount).sum::<u64>()
        + offered_cats.iter().map(|cat| cat.coin.amount).sum::<u64>();
    if offered_total < request.offer_amount {
        return Err(SignerError::InsufficientOfferCoinTotal);
    }
    let change_amount = offered_total.saturating_sub(request.offer_amount);

    let offered_coin_ids: Vec<Bytes32> = offered_xch
        .iter()
        .map(|coin| coin.coin_id())
        .chain(offered_cats.iter().map(|cat| cat.coin.coin_id()))
        .collect();
    let offer_nonce = Offer::nonce(offered_coin_ids.clone());

    let keys = synthetic_keys_for_coins(master_sk, &offered_xch, &offered_cats)?;

    let terms = OfferTerms {
        receive_address: receive_address.to_string(),
        offer_asset_id: offer_asset_raw.clone(),
        offer_amount: request.offer_amount,
        request_asset_id: request.request_asset_id.trim().to_lowercase(),
        request_amount: request.request_amount,
        expires_at: None,
    };

    let mut ctx = SpendContext::new();
    let requested_payments =
        build_requested_payments(&mut ctx, &terms, receive_puzzle_hash, offer_nonce)?;
    let requested_asset_info = AssetInfo::new();

    let mut spends = Spends::new(receive_puzzle_hash);
    add_coins_to_spends(&mut spends, offered_xch, offered_cats);

    let offer_id = if is_xch_like_asset(&offer_asset_raw) {
        Id::Xch
    } else {
        Id::Existing(cat_asset_bytes(&offer_asset_raw)?)
    };
    let mut actions = vec![Action::send(
        offer_id,
        SETTLEMENT_PAYMENT_HASH.into(),
        request.offer_amount,
        Memos::None,
    )];
    if change_amount > 0 {
        actions.push(Action::send(
            offer_id,
            receive_puzzle_hash,
            change_amount,
            Memos::None,
        ));
    }

    spends.conditions.required = spends.conditions.required.extend(
        requested_payments
            .assertions(&mut ctx, &requested_asset_info)
            .map_err(SignerError::from)?,
    );

    let deltas = spends.apply(&mut ctx, &actions)?;
    spends.finish_with_keys(&mut ctx, &deltas, Relation::None, &keys.synthetic_pks)?;
    let coin_spends = ctx.take();
    let signature = sign_coin_spends(network, &coin_spends, &keys.synthetic_sks)?;
    let input_spend_bundle = SpendBundle::new(coin_spends, signature);

    let mut allocator = Allocator::new();
    let offer = Offer::from_input_spend_bundle(
        &mut allocator,
        input_spend_bundle,
        requested_payments,
        requested_asset_info,
    )
    .map_err(SignerError::from)?;
    let offer_spend_bundle = offer.to_spend_bundle(&mut ctx).map_err(SignerError::from)?;

    Ok(BlsOfferResult {
        spend_bundle_hex: crate::coinset::spend_bundle_hex(&offer_spend_bundle)?,
    })
}
