use chia_bls::SecretKey;
use chia_protocol::{Bytes32, Coin};
use chia_puzzle_types::Memos;
use chia_puzzles::SETTLEMENT_PAYMENT_HASH;
use chia_sdk_driver::{Action, AssetInfo, Cat, Id, Offer, SpendContext};
use crate::coinset::CoinsetClient;
use clvmr::Allocator;
use serde::{Deserialize, Serialize};

use crate::bls::coins::cat_asset_bytes;
use crate::bls::spend::build_signed_spend;
use crate::coinset::is_xch_like_asset;
use crate::coinset::{
    client_for_network, list_and_select_cats, select_xch_for_amount, CatSelectionMode,
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

async fn select_offer_inputs(
    client: &CoinsetClient,
    receive_address: &str,
    offer_asset_raw: &str,
    explicit_coin_ids: &[Bytes32],
    offer_amount: u64,
) -> SignerResult<(Vec<Coin>, Vec<Cat>, u64)> {
    if is_xch_like_asset(offer_asset_raw) {
        let offered_xch = select_xch_for_amount(
            client,
            receive_address,
            explicit_coin_ids,
            offer_amount,
            SignerError::NoUnspentOfferXchCoins,
            SignerError::InsufficientOfferXchCoins,
        )
        .await?;
        let offered_total: u64 = offered_xch.iter().map(|c| c.amount).sum();
        return Ok((offered_xch, Vec::new(), offered_total.saturating_sub(offer_amount)));
    }
    let asset_bytes = cat_asset_bytes(offer_asset_raw)?;
    let offered_cats = list_and_select_cats(
        client,
        receive_address,
        asset_bytes,
        explicit_coin_ids,
        offer_amount,
        CatSelectionMode::SmallestFirst,
        SignerError::NoUnspentOfferCatCoins,
        SignerError::InsufficientOfferCatCoins,
    )
    .await?;
    let offered_total: u64 = offered_cats.iter().map(|cat| cat.coin.amount).sum();
    Ok((Vec::new(), offered_cats, offered_total.saturating_sub(offer_amount)))
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

    let (offered_xch, offered_cats, change_amount) = select_offer_inputs(
        &client,
        receive_address,
        &offer_asset_raw,
        &explicit_coin_ids,
        request.offer_amount,
    )
    .await?;

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

    let offered_coin_ids: Vec<_> = offered_xch
        .iter()
        .map(|coin| coin.coin_id())
        .chain(offered_cats.iter().map(|cat| cat.coin.coin_id()))
        .collect();
    let offer_nonce = Offer::nonce(offered_coin_ids);

    let terms = OfferTerms {
        receive_address: receive_address.to_string(),
        offer_asset_id: offer_asset_raw,
        offer_amount: request.offer_amount,
        request_asset_id: request.request_asset_id.trim().to_lowercase(),
        request_amount: request.request_amount,
        expires_at: None,
    };

    let mut payments_ctx = SpendContext::new();
    let requested_payments =
        build_requested_payments(&mut payments_ctx, &terms, receive_puzzle_hash, offer_nonce)?;
    let requested_asset_info = AssetInfo::new();

    let (input_spend_bundle, mut ctx) = build_signed_spend(
        network,
        receive_puzzle_hash,
        offered_xch,
        offered_cats,
        actions,
        master_sk,
        |spends, ctx| {
            spends.conditions.required = spends.conditions.required.clone().extend(
                requested_payments
                    .assertions(ctx, &requested_asset_info)
                    .map_err(SignerError::from)?,
            );
            Ok(())
        },
    )?;

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
