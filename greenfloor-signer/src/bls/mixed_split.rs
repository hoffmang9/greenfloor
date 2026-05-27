use chia_bls::SecretKey;
use chia_protocol::{Coin, SpendBundle};
use chia_puzzle_types::Memos;
use chia_sdk_driver::{Action, Cat, Id};
use chia_traits::Streamable;
use chia_sdk_utils::select_coins;
use serde::{Deserialize, Serialize};

use crate::bls::coins::cat_asset_bytes;
use crate::bls::select::select_xch_for_amount;
use crate::bls::spend::build_signed_standard_spend;
use crate::coinset::is_xch_like_asset;
use crate::coinset::{
    broadcast_spend_bundle, client_for_network, list_and_select_cats, list_unspent_xch,
    BroadcastSpendBundleResult, CatSelectionMode, MIN_CAT_OUTPUT_MOJOS,
};
use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone, Deserialize)]
pub struct BlsMixedSplitRequest {
    pub receive_address: String,
    pub asset_id: String,
    pub output_amounts: Vec<u64>,
    #[serde(default)]
    pub coin_ids: Vec<String>,
    #[serde(default)]
    pub allow_sub_cat_output: bool,
    #[serde(default)]
    pub fee_mojos: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlsMixedSplitResult {
    pub spend_bundle_hex: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_coin_ids: Vec<String>,
}

pub async fn build_bls_mixed_split_spend_bundle(
    network: &str,
    master_sk: &SecretKey,
    request: BlsMixedSplitRequest,
) -> SignerResult<BlsMixedSplitResult> {
    if request.output_amounts.is_empty() {
        return Err(SignerError::MissingOutputAmounts);
    }
    if request.output_amounts.iter().any(|amount| *amount == 0) {
        return Err(SignerError::InvalidOutputAmount);
    }
    let allow_sub_cat_output = request.allow_sub_cat_output;
    let asset_raw = request.asset_id.trim().to_lowercase();
    if !allow_sub_cat_output
        && !is_xch_like_asset(&asset_raw)
        && request
            .output_amounts
            .iter()
            .any(|amount| *amount < MIN_CAT_OUTPUT_MOJOS)
    {
        return Err(SignerError::CatOutputBelowMinimum);
    }

    let client = client_for_network(network)?;
    let receive_address = request.receive_address.trim();
    let receive_puzzle_hash =
        crate::coinset::decode_receive_address(receive_address)?;
    let target_total: u64 = request.output_amounts.iter().sum();
    let fee_mojos = request.fee_mojos;
    let explicit_coin_ids = crate::coinset::parse_coin_ids(&request.coin_ids)?;

    let mut offered_xch: Vec<Coin> = Vec::new();
    let mut offered_cats: Vec<Cat> = Vec::new();
    let mut fee_xch: Vec<Coin> = Vec::new();
    let mut selected_coin_ids: Vec<String> = Vec::new();

    if is_xch_like_asset(&asset_raw) {
        let required_total = target_total.saturating_add(fee_mojos);
        offered_xch = select_xch_for_amount(
            &client,
            receive_address,
            &explicit_coin_ids,
            required_total,
            SignerError::NoUnspentXchCoins,
            SignerError::XchCoinSelectionFailed,
        )
        .await?;
    } else {
        let asset_bytes = cat_asset_bytes(&asset_raw)?;
        let cat_mode = if explicit_coin_ids.is_empty() {
            CatSelectionMode::SmallestFirst
        } else {
            CatSelectionMode::ExplicitSum
        };
        offered_cats = list_and_select_cats(
            &client,
            receive_address,
            asset_bytes,
            &explicit_coin_ids,
            target_total,
            cat_mode,
            SignerError::InsufficientCatCoins,
            SignerError::InsufficientCatCoins,
        )
        .await?;
        if fee_mojos > 0 {
            let xch_coins = list_unspent_xch(&client, receive_address).await?;
            let available: u64 = xch_coins.iter().map(|coin| coin.amount).sum();
            if available < fee_mojos {
                return Err(SignerError::InsufficientXchFeeBalanceForMixedSplit);
            }
            fee_xch = select_coins(xch_coins, fee_mojos)
                .map_err(|_| SignerError::InsufficientXchFeeBalanceForMixedSplit)?;
        }
    }

    let offered_total: u64 = offered_xch.iter().map(|c| c.amount).sum::<u64>()
        + offered_cats.iter().map(|cat| cat.coin.amount).sum::<u64>();
    let fee_xch_total: u64 = fee_xch.iter().map(|c| c.amount).sum::<u64>();
    if offered_total < target_total {
        return Err(SignerError::InsufficientOfferedTotalForMixedSplit);
    }
    if !is_xch_like_asset(&asset_raw) && fee_mojos > fee_xch_total {
        return Err(SignerError::InsufficientXchFeeBalanceForMixedSplit);
    }

    for coin in &offered_xch {
        selected_coin_ids.push(format!("0x{}", hex::encode(coin.coin_id())));
    }
    for cat in &offered_cats {
        selected_coin_ids.push(format!("0x{}", hex::encode(cat.coin.coin_id())));
    }
    for coin in &fee_xch {
        selected_coin_ids.push(format!("0x{}", hex::encode(coin.coin_id())));
    }

    let asset_id = if is_xch_like_asset(&asset_raw) {
        Id::Xch
    } else {
        Id::Existing(cat_asset_bytes(&asset_raw)?)
    };

    let mut actions = Vec::new();
    for amount in &request.output_amounts {
        actions.push(Action::send(
            asset_id,
            receive_puzzle_hash,
            *amount,
            Memos::None,
        ));
    }
    let mut offered_change = offered_total.saturating_sub(target_total);
    if is_xch_like_asset(&asset_raw) {
        offered_change = offered_change.saturating_sub(fee_mojos);
    }
    if !allow_sub_cat_output
        && !is_xch_like_asset(&asset_raw)
        && offered_change > 0
        && offered_change < MIN_CAT_OUTPUT_MOJOS
    {
        return Err(SignerError::CatChangeBelowMinimum);
    }
    if offered_change > 0 {
        actions.push(Action::send(
            asset_id,
            receive_puzzle_hash,
            offered_change,
            Memos::None,
        ));
    }
    let fee_change = fee_xch_total.saturating_sub(fee_mojos);
    if fee_change > 0 {
        actions.push(Action::send(
            Id::Xch,
            receive_puzzle_hash,
            fee_change,
            Memos::None,
        ));
    }

    let mut input_xch = offered_xch;
    input_xch.extend(fee_xch);
    let spend_bundle = build_signed_standard_spend(
        network,
        receive_puzzle_hash,
        input_xch,
        offered_cats,
        actions,
        master_sk,
    )?;
    Ok(BlsMixedSplitResult {
        spend_bundle_hex: crate::coinset::spend_bundle_hex(&spend_bundle)?,
        selected_coin_ids,
    })
}

pub async fn broadcast_bls_spend_bundle(
    network: &str,
    spend_bundle_hex: &str,
) -> SignerResult<BroadcastSpendBundleResult> {
    let client = client_for_network(network)?;
    let raw = spend_bundle_hex.trim().trim_start_matches("0x");
    let bytes = hex::decode(raw).map_err(|err| SignerError::Other(format!("invalid hex: {err}")))?;
    let spend_bundle =
        SpendBundle::from_bytes(&bytes)
            .map_err(|err: chia_traits::Error| SignerError::Other(err.to_string()))?;
    broadcast_spend_bundle(&client, spend_bundle).await
}
