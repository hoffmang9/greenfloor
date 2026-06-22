//! Coinset spendable coin scans shared by inventory and offer-dispatch paths.

use std::collections::{BTreeMap, BTreeSet};

use crate::coinset::list_wallet_unspent_coins_for_signer;
use crate::config::SignerConfig;
use crate::cycle::SpendableAssetProfile;
use crate::error::SignerResult;

pub async fn list_spendable_base_unit_amounts_for_signer(
    network: &str,
    signer: &SignerConfig,
    receive_address: &str,
    resolved_asset_id: &str,
    base_unit_multiplier: i64,
) -> SignerResult<Vec<i64>> {
    let coins =
        list_wallet_unspent_coins_for_signer(network, signer, receive_address, resolved_asset_id)
            .await?;
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

pub async fn coinset_spendable_profiles_for_signer(
    network: &str,
    signer: &SignerConfig,
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
        let coins =
            list_wallet_unspent_coins_for_signer(network, signer, receive_address, asset_id)
                .await?;
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        coinset_spendable_profiles_for_signer, list_spendable_base_unit_amounts_for_signer,
    };
    use crate::test_support::signer_config::test_signer_config;

    #[tokio::test]
    async fn list_spendable_base_unit_amounts_scales_confirmed_coins() {
        const RECEIVE_ADDRESS: &str =
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
        let body = r#"{
        "success": true,
        "coin_records": [{
            "coin": {
                "parent_coin_info": "c325057d788bee13367cb8e2d71ff3e209b5e94b31b296322ba1a143053fef5b",
                "puzzle_hash": "11cd056d9ec93f4612919b445e1ad9afeb7ef7739708c2d16cec4fd2d3cd5e63",
                "amount": 5000
            },
            "coinbase": false,
            "confirmed_block_index": 1,
            "spent": false,
            "spent_block_index": 0,
            "timestamp": 1
        }]
    }"#;
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let signer = test_signer_config(&server.url());
        let amounts = list_spendable_base_unit_amounts_for_signer(
            "mainnet",
            &signer,
            RECEIVE_ADDRESS,
            "xch",
            1_000,
        )
        .await
        .expect("amounts");
        assert_eq!(amounts, vec![5]);
    }

    #[tokio::test]
    async fn spendable_profiles_propagates_coin_list_lookup_errors() {
        const RECEIVE_ADDRESS: &str =
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(500)
            .create_async()
            .await;

        let signer = test_signer_config(&server.url());
        let assets = BTreeSet::from(["xch".to_string()]);
        let err =
            coinset_spendable_profiles_for_signer("mainnet", &signer, RECEIVE_ADDRESS, &assets)
                .await
                .expect_err("lookup error");
        assert!(
            !err.to_string().is_empty(),
            "expected propagated coin-list error"
        );
    }

    #[tokio::test]
    async fn spendable_profiles_empty_when_asset_set_or_address_missing() {
        let signer = test_signer_config("https://example.test");
        let empty_assets = BTreeSet::new();
        let profiles =
            coinset_spendable_profiles_for_signer("mainnet", &signer, "xch1test", &empty_assets)
                .await
                .expect("profiles");
        assert!(profiles.is_empty());

        let assets = BTreeSet::from(["asset-1".to_string()]);
        let profiles = coinset_spendable_profiles_for_signer("mainnet", &signer, "  ", &assets)
            .await
            .expect("profiles");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles["asset-1"].total, 0);
        assert_eq!(profiles["asset-1"].max_single, 0);
    }
}
