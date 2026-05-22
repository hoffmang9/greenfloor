use chia_protocol::Bytes32;
use chia_puzzle_types::{
    Memos,
    offer::{NotarizedPayment, Payment},
};
use chia_puzzles::SETTLEMENT_PAYMENT_HASH;
use chia_sdk_driver::{
    Action, AssetInfo, Cat, DriverError, Id, Offer, RequestedPayments, Spends, encode_offer,
};
use clvmr::Allocator;

use crate::coinset;
use chia_sdk_coinset::CoinsetClient;
use crate::config::CloudWalletConfig;
use crate::error::{SignerError, SignerResult};
use crate::offer::presplit::{
    build_fixed_presplit_conditions_spend, build_offer_from_presplit_cat,
    build_presplit_split_spend_bundle, fetch_presplit_cat_by_id, predict_presplit_cat,
    should_presplit, vault_change_puzzle_hash,
};
use crate::vault::members::{hex_to_bytes32, p2_conditions_or_singleton_puzzle_hash};
use crate::vault::spend::{materialize_vault_cat_finished_spends, resolve_vault_spend_context};

const XCH_LIKE_ASSETS: [&str; 4] = ["", "xch", "txch", "1"];

#[derive(Debug, Clone)]
pub struct CreateOfferRequest {
    pub receive_address: String,
    pub offer_asset_id: String,
    pub offer_amount: u64,
    pub request_asset_id: String,
    pub request_amount: u64,
    pub offer_coin_ids: Vec<Bytes32>,
    pub presplit_coin_ids: Vec<Bytes32>,
    pub split_input_coins: bool,
    pub broadcast_split: bool,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CreateOfferResult {
    pub offer: String,
    pub spend_bundle_hex: String,
    pub selected_coin_ids: Vec<String>,
    pub offer_nonce: String,
    pub needs_presplit: bool,
    pub split_spend_bundle_hex: Option<String>,
    pub presplit_coin_id: Option<String>,
    pub split_broadcast_status: Option<String>,
}

pub async fn build_vault_cat_offer(
    config: CloudWalletConfig,
    request: CreateOfferRequest,
) -> SignerResult<CreateOfferResult> {
    if request.offer_amount == 0 || request.request_amount == 0 {
        return Err(SignerError::InvalidOutputAmount);
    }
    if is_xch_like(&request.offer_asset_id) {
        return Err(SignerError::Other(
            "vault local offer path supports CAT offer side only".to_string(),
        ));
    }

    let mut vault_ctx = resolve_vault_spend_context(config.clone()).await?;
    let coinset = coinset::client_for_network(&vault_ctx.network)?;
    let receive_puzzle_hash = chia_sdk_utils::Address::decode(&request.receive_address)
        .map_err(|err| SignerError::Other(format!("invalid receive address: {err}")))?
        .puzzle_hash;
    let offer_asset_id = hex_to_bytes32(&request.offer_asset_id)?;

    if !request.presplit_coin_ids.is_empty() {
        return build_offer_from_existing_presplit_coins(
            &coinset,
            &request,
            receive_puzzle_hash,
            offer_asset_id,
            vault_ctx.launcher_id,
        )
        .await;
    }

    let cats = if request.offer_coin_ids.is_empty() {
        coinset::list_unspent_cats(&coinset, &request.receive_address, offer_asset_id).await?
    } else {
        coinset::list_unspent_cats_by_ids(&coinset, &request.offer_coin_ids).await?
    };
    if cats.is_empty() {
        return Err(SignerError::NoUnspentCatCoins);
    }
    let selected = if request.offer_coin_ids.is_empty() {
        coinset::select_cats_smallest_first(cats, request.offer_amount)
    } else {
        cats
    };
    if selected.is_empty() {
        return Err(SignerError::InsufficientCatCoins);
    }
    let offered_total: u64 = selected.iter().map(|cat| cat.coin.amount).sum();
    if offered_total < request.offer_amount {
        return Err(SignerError::InsufficientCatCoins);
    }

    let change_amount = offered_total - request.offer_amount;
    if offer_input_requires_presplit(offered_total, request.offer_amount, request.split_input_coins) {
        return Err(SignerError::OfferInputRequiresPresplit);
    }

    if should_presplit(offered_total, request.offer_amount, request.split_input_coins) {
        return build_presplit_vault_cat_offer(
            &mut vault_ctx,
            &coinset,
            request,
            receive_puzzle_hash,
            selected,
            change_amount,
        )
        .await;
    }

    build_direct_vault_cat_offer(
        &mut vault_ctx,
        &coinset,
        request,
        receive_puzzle_hash,
        offer_asset_id,
        selected,
        offered_total,
    )
    .await
}

async fn build_presplit_vault_cat_offer(
    vault_ctx: &mut crate::vault::spend::VaultSpendContext,
    coinset: &CoinsetClient,
    request: CreateOfferRequest,
    receive_puzzle_hash: Bytes32,
    selected: Vec<Cat>,
    change_amount: u64,
) -> SignerResult<CreateOfferResult> {
    let mut planning_ctx = chia_sdk_driver::SpendContext::new();
    let placeholder_nonce = Bytes32::default();
    let requested_payments = build_requested_payments(
        &mut planning_ctx,
        &request,
        receive_puzzle_hash,
        placeholder_nonce,
    )?;
    let requested_asset_info = AssetInfo::new();
    let fixed_spend = build_fixed_presplit_conditions_spend(
        &mut planning_ctx,
        &requested_payments,
        &requested_asset_info,
        request.offer_amount,
        request.expires_at,
    )?;
    let p2_hashes = p2_conditions_or_singleton_puzzle_hash(
        planning_ctx.tree_hash(fixed_spend.puzzle),
        vault_ctx.launcher_id,
    );
    let change_puzzle_hash = vault_change_puzzle_hash(vault_ctx.launcher_id);
    let predicted_presplit_cat = predict_presplit_cat(
        &selected[0],
        p2_hashes.puzzle_hash.into(),
        request.offer_amount,
    );

    let (split_spend_bundle, _) = build_presplit_split_spend_bundle(
        vault_ctx,
        coinset,
        &selected,
        change_puzzle_hash,
        p2_hashes.puzzle_hash.into(),
        request.offer_amount,
        change_amount,
    )
    .await?;
    let split_spend_bundle_hex = coinset::spend_bundle_hex(&split_spend_bundle)?;
    let split_broadcast_status = if request.broadcast_split {
        Some(coinset::broadcast_spend_bundle(coinset, split_spend_bundle).await?)
    } else {
        None
    };

    let presplit_cat = if request.broadcast_split {
        coinset::wait_for_unspent_cat(coinset, predicted_presplit_cat.coin.coin_id()).await?
    } else {
        predicted_presplit_cat
    };
    let offer_nonce = Offer::nonce(vec![presplit_cat.coin.coin_id()]);
    let requested_payments =
        build_requested_payments(&mut planning_ctx, &request, receive_puzzle_hash, offer_nonce)?;
    let (offer, spend_bundle_hex, offer_nonce_hex) = build_offer_from_presplit_cat(
        presplit_cat,
        vault_ctx.launcher_id,
        requested_payments,
        requested_asset_info,
        request.offer_amount,
        request.expires_at,
    )
    .await?;

    Ok(CreateOfferResult {
        offer,
        spend_bundle_hex,
        selected_coin_ids: selected
            .iter()
            .map(|cat| hex::encode(cat.coin.coin_id()))
            .collect(),
        offer_nonce: offer_nonce_hex,
        needs_presplit: true,
        split_spend_bundle_hex: Some(split_spend_bundle_hex),
        presplit_coin_id: Some(hex::encode(presplit_cat.coin.coin_id())),
        split_broadcast_status,
    })
}

async fn build_offer_from_existing_presplit_coins(
    coinset: &CoinsetClient,
    request: &CreateOfferRequest,
    receive_puzzle_hash: Bytes32,
    offer_asset_id: Bytes32,
    launcher_id: Bytes32,
) -> SignerResult<CreateOfferResult> {
    if request.presplit_coin_ids.len() != 1 {
        return Err(SignerError::Other(
            "presplit offer path supports exactly one presplit coin".to_string(),
        ));
    }
    let presplit_cat = fetch_presplit_cat_by_id(coinset, request.presplit_coin_ids[0]).await?;
    if presplit_cat.info.asset_id != offer_asset_id {
        return Err(SignerError::Other(
            "presplit coin asset id does not match offer asset id".to_string(),
        ));
    }
    if presplit_cat.coin.amount != request.offer_amount {
        return Err(SignerError::Other(format!(
            "presplit coin amount {} does not match offer amount {}",
            presplit_cat.coin.amount, request.offer_amount
        )));
    }

    let mut planning_ctx = chia_sdk_driver::SpendContext::new();
    let offer_nonce = Offer::nonce(vec![presplit_cat.coin.coin_id()]);
    let requested_payments =
        build_requested_payments(&mut planning_ctx, request, receive_puzzle_hash, offer_nonce)?;
    let requested_asset_info = AssetInfo::new();
    let (offer, spend_bundle_hex, offer_nonce_hex) = build_offer_from_presplit_cat(
        presplit_cat,
        launcher_id,
        requested_payments,
        requested_asset_info,
        request.offer_amount,
        request.expires_at,
    )
    .await?;

    Ok(CreateOfferResult {
        offer,
        spend_bundle_hex,
        selected_coin_ids: vec![hex::encode(presplit_cat.coin.coin_id())],
        offer_nonce: offer_nonce_hex,
        needs_presplit: true,
        split_spend_bundle_hex: None,
        presplit_coin_id: Some(hex::encode(presplit_cat.coin.coin_id())),
        split_broadcast_status: None,
    })
}

async fn build_direct_vault_cat_offer(
    vault_ctx: &mut crate::vault::spend::VaultSpendContext,
    coinset: &CoinsetClient,
    request: CreateOfferRequest,
    receive_puzzle_hash: Bytes32,
    offer_asset_id: Bytes32,
    selected: Vec<Cat>,
    offered_total: u64,
) -> SignerResult<CreateOfferResult> {
    let mut ctx = chia_sdk_driver::SpendContext::new();
    let mut spends = Spends::new(receive_puzzle_hash);
    for cat in &selected {
        spends.add(*cat);
    }

    let offer_id = Id::Existing(offer_asset_id);
    let mut actions = vec![Action::send(
        offer_id,
        SETTLEMENT_PAYMENT_HASH.into(),
        request.offer_amount,
        Memos::None,
    )];
    let offer_change = offered_total - request.offer_amount;
    if offer_change > 0 {
        actions.push(Action::send(
            offer_id,
            receive_puzzle_hash,
            offer_change,
            Memos::None,
        ));
    }

    let offered_coin_ids: Vec<Bytes32> = selected.iter().map(|cat| cat.coin.coin_id()).collect();
    let offer_nonce = Offer::nonce(offered_coin_ids);
    let requested_payments =
        build_requested_payments(&mut ctx, &request, receive_puzzle_hash, offer_nonce)?;
    let requested_asset_info = AssetInfo::new();
    spends.conditions.required = spends.conditions.required.extend(
        requested_payments.assertions(&mut ctx, &requested_asset_info)?,
    );

    let deltas = spends.apply(&mut ctx, &actions).map_err(driver_err)?;
    let finished = spends
        .prepare(&mut ctx, &deltas, chia_sdk_driver::Relation::None)
        .map_err(driver_err)?;

    let input_spend_bundle =
        materialize_vault_cat_finished_spends(&mut ctx, vault_ctx, coinset, finished).await?;

    let mut allocator = Allocator::new();
    let offer = Offer::from_input_spend_bundle(
        &mut allocator,
        input_spend_bundle.clone(),
        requested_payments,
        requested_asset_info,
    )
    .map_err(driver_err)?;
    let offer_spend_bundle = offer.to_spend_bundle(&mut ctx).map_err(driver_err)?;
    let offer_text = encode_offer(&offer_spend_bundle).map_err(driver_err)?;
    let spend_bundle_hex = coinset::spend_bundle_hex(&offer_spend_bundle)?;

    Ok(CreateOfferResult {
        offer: offer_text,
        spend_bundle_hex,
        selected_coin_ids: selected
            .iter()
            .map(|cat| hex::encode(cat.coin.coin_id()))
            .collect(),
        offer_nonce: hex::encode(offer_nonce),
        needs_presplit: false,
        split_spend_bundle_hex: None,
        presplit_coin_id: None,
        split_broadcast_status: None,
    })
}

fn build_requested_payments(
    ctx: &mut chia_sdk_driver::SpendContext,
    request: &CreateOfferRequest,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
) -> SignerResult<RequestedPayments> {
    let mut requested_payments = RequestedPayments::new();
    if is_xch_like(&request.request_asset_id) {
        requested_payments.xch.push(NotarizedPayment::new(
            offer_nonce,
            vec![Payment::new(
                receive_puzzle_hash,
                request.request_amount,
                Memos::None,
            )],
        ));
        return Ok(requested_payments);
    }

    let request_asset_id = hex_to_bytes32(&request.request_asset_id)?;
    let memos = ctx.hint(receive_puzzle_hash).map_err(driver_err)?;
    requested_payments.cats.insert(
        request_asset_id,
        vec![NotarizedPayment::new(
            offer_nonce,
            vec![Payment::new(
                receive_puzzle_hash,
                request.request_amount,
                memos,
            )],
        )],
    );
    Ok(requested_payments)
}

fn is_xch_like(asset_id: &str) -> bool {
    let normalized = asset_id.trim().to_ascii_lowercase();
    XCH_LIKE_ASSETS.contains(&normalized.as_str())
}

pub(crate) fn offer_input_requires_presplit(
    offered_total: u64,
    offer_amount: u64,
    split_input_coins: bool,
) -> bool {
    offered_total > offer_amount && !split_input_coins
}

fn driver_err(err: DriverError) -> SignerError {
    SignerError::Driver(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_xch_like_assets() {
        assert!(is_xch_like("xch"));
        assert!(is_xch_like("TXCH"));
        assert!(!is_xch_like("aa".repeat(32).as_str()));
    }

    #[test]
    fn offer_input_requires_presplit_when_change_without_flag() {
        assert!(offer_input_requires_presplit(5000, 1000, false));
        assert!(!offer_input_requires_presplit(5000, 1000, true));
        assert!(!offer_input_requires_presplit(1000, 1000, false));
    }
}
