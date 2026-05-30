use std::collections::{BTreeMap, BTreeSet};

use crate::coinset::list_wallet_unspent_coins;
use crate::cycle::SpendableAssetProfile;
use crate::error::SignerResult;

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
        let coins = match list_wallet_unspent_coins(network, receive_address, asset_id).await {
            Ok(coins) => coins,
            Err(_) => continue,
        };
        for coin in coins {
            let amount = i64::try_from(coin.amount).unwrap_or(0);
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
