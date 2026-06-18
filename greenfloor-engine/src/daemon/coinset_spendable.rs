//! Coinset spendable coin scans shared by inventory and offer-dispatch paths.

use std::collections::{BTreeMap, BTreeSet};

use crate::coinset::list_wallet_unspent_coins;
use crate::cycle::SpendableAssetProfile;
use crate::error::SignerResult;

pub async fn list_spendable_base_unit_amounts(
    network: &str,
    receive_address: &str,
    resolved_asset_id: &str,
    base_unit_multiplier: i64,
) -> SignerResult<Vec<i64>> {
    let coins = list_wallet_unspent_coins(network, receive_address, resolved_asset_id).await?;
    let multiplier = base_unit_multiplier.max(1);
    Ok(coins
        .into_iter()
        .filter_map(|coin| {
            let amount_mojos = i64::try_from(coin.amount).ok()?;
            if amount_mojos <= 0 {
                return None;
            }
            let base_units = amount_mojos / multiplier;
            (base_units > 0).then_some(base_units)
        })
        .collect())
}

pub async fn coinset_spendable_profiles_by_asset(
    network: &str,
    receive_address: &str,
    asset_ids: &BTreeSet<String>,
) -> SignerResult<BTreeMap<String, SpendableAssetProfile>> {
    let receive_address = receive_address.trim();
    let mut profiles: BTreeMap<String, SpendableAssetProfile> = asset_ids
        .iter()
        .map(|asset_id| {
            (
                asset_id.clone(),
                SpendableAssetProfile {
                    total: 0,
                    max_single: 0,
                    max_single_known: true,
                },
            )
        })
        .collect();
    if asset_ids.is_empty() || receive_address.is_empty() {
        return Ok(profiles);
    }
    for asset_id in asset_ids {
        let profile = profiles.get_mut(asset_id).expect("profile");
        let Ok(coins) = list_wallet_unspent_coins(network, receive_address, asset_id).await else {
            continue;
        };
        for coin in coins {
            let Some(amount) = i64::try_from(coin.amount).ok() else {
                continue;
            };
            if amount <= 0 {
                continue;
            }
            profile.total += amount;
            if amount > profile.max_single {
                profile.max_single = amount;
            }
        }
    }
    Ok(profiles)
}
