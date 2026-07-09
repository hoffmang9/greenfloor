use serde_json::Value;

use crate::coin_ops::is_spendable_coin_state;
use crate::coinset::{get_conservative_fee_estimate_for_signer, WalletUnspentCoin};
use crate::config::{LadderEntry, SignerConfig};
use crate::error::SignerResult;
use crate::offer::bootstrap::{BaseUnits, BootstrapCoin, PlannerLadderRow};
use crate::offer::build_context::mojo_multiplier_for_leg;
use crate::offer::pricing::quote_mojos_for_base_size;
use crate::offer::request::normalize_offer_side;

pub(super) fn bootstrap_ladder_entries_for_side(
    side: &str,
    side_ladder: &[LadderEntry],
    pricing: &Value,
    quote_price: f64,
    resolved_quote_asset_id: &str,
) -> SignerResult<Vec<PlannerLadderRow>> {
    let side = normalize_offer_side(side);
    let mut quote_unit_multiplier: Option<i64> = None;
    if side == "buy" {
        quote_unit_multiplier = Some(mojo_multiplier_for_leg(
            pricing,
            "quote_unit_mojo_multiplier",
            resolved_quote_asset_id,
        ));
    }
    let mut entries = Vec::new();
    for entry in side_ladder {
        let mut size_base_units = entry.size_base_units;
        if let Some(multiplier) = quote_unit_multiplier {
            size_base_units = quote_mojos_for_base_size(size_base_units, quote_price, multiplier)?;
            if size_base_units <= 0 {
                continue;
            }
        }
        entries.push(PlannerLadderRow {
            size_base_units,
            target_count: entry.target_count,
            split_buffer_count: entry.split_buffer_count,
        });
    }
    Ok(entries)
}

fn bootstrap_fee_cost_for_output_count(output_count: usize) -> u64 {
    let count = u64::try_from(output_count.max(1)).unwrap_or(u64::MAX);
    1_000_000 + count.saturating_sub(1) * 250_000
}

pub(super) async fn resolve_bootstrap_split_fee(
    signer: &SignerConfig,
    operator_network: &str,
    minimum_fee_mojos: u64,
    output_count: usize,
) -> (u64, String, Option<String>) {
    let fee_cost = bootstrap_fee_cost_for_output_count(output_count);
    let spend_count = u64::try_from(output_count.max(1)).unwrap_or(u64::MAX);
    match get_conservative_fee_estimate_for_signer(
        signer,
        operator_network,
        fee_cost,
        Some(spend_count),
    )
    .await
    {
        Ok(Some(fee_mojos)) => (fee_mojos, "coinset_conservative_fee".to_string(), None),
        Ok(None) => (
            minimum_fee_mojos,
            "config_minimum_fee_fallback".to_string(),
            None,
        ),
        Err(err) => (
            minimum_fee_mojos,
            "config_minimum_fee_fallback".to_string(),
            Some(err.to_string()),
        ),
    }
}

pub(super) fn wallet_coin_spendable(coin: &WalletUnspentCoin) -> bool {
    is_spendable_coin_state(&coin.state)
}

/// Map on-chain coin amounts (mojos) to ladder `size_base_units` for bootstrap planning.
pub(super) fn bootstrap_coins_in_base_units(
    coins: &[WalletUnspentCoin],
    mojo_multiplier: i64,
) -> Vec<BootstrapCoin> {
    let multiplier = mojo_multiplier.max(1);
    coins
        .iter()
        .filter(|coin| wallet_coin_spendable(coin))
        .filter_map(|coin| {
            let amount_mojos = i64::try_from(coin.amount).ok()?;
            let base_units = amount_mojos / multiplier;
            (base_units > 0).then(|| BootstrapCoin {
                id: coin.id.clone(),
                amount: BaseUnits::new(base_units),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        bootstrap_ladder_entries_for_side, resolve_bootstrap_split_fee, wallet_coin_spendable,
    };
    use crate::coinset::WalletUnspentCoin;
    use crate::config::LadderEntry;
    use crate::test_support::signer_config::test_signer_config;

    #[test]
    fn bootstrap_ladder_entries_for_sell_side_preserves_sizes() {
        let ladder = vec![LadderEntry {
            size_base_units: 25,
            target_count: 3,
            split_buffer_count: 1,
            combine_when_excess_factor: 2.0,
        }];
        let entries = bootstrap_ladder_entries_for_side("sell", &ladder, &json!({}), 1.0, "xch")
            .expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size_base_units, 25);
        assert_eq!(entries[0].target_count, 3);
    }

    #[test]
    fn bootstrap_ladder_entries_for_buy_side_converts_quote_sizes() {
        let ladder = vec![LadderEntry {
            size_base_units: 10,
            target_count: 2,
            split_buffer_count: 0,
            combine_when_excess_factor: 2.0,
        }];
        let pricing = json!({"quote_unit_mojo_multiplier": 1000});
        let entries = bootstrap_ladder_entries_for_side(
            "buy",
            &ladder,
            &pricing,
            2.0,
            "0000000000000000000000000000000000000000000000000000000000000001",
        )
        .expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size_base_units, 20_000);
    }

    #[test]
    fn bootstrap_coins_in_base_units_divides_cat_mojos() {
        use super::bootstrap_coins_in_base_units;
        use crate::offer::bootstrap::BaseUnits;

        let coins = vec![
            WalletUnspentCoin {
                id: "a".repeat(64),
                name: "a".repeat(64),
                amount: 5_000,
                state: "CONFIRMED".to_string(),
                puzzle_hash: String::new(),
            },
            WalletUnspentCoin {
                id: "b".repeat(64),
                name: "b".repeat(64),
                amount: 500,
                state: "CONFIRMED".to_string(),
                puzzle_hash: String::new(),
            },
        ];
        let base = bootstrap_coins_in_base_units(&coins, 1_000);
        assert_eq!(base.len(), 1);
        assert_eq!(base[0].amount, BaseUnits::new(5));
    }

    #[test]
    fn wallet_coin_spendable_requires_confirmed_state() {
        let confirmed = WalletUnspentCoin {
            id: "a".repeat(64),
            name: "a".repeat(64),
            amount: 1,
            state: "CONFIRMED".to_string(),
            puzzle_hash: String::new(),
        };
        let pending = WalletUnspentCoin {
            id: "b".repeat(64),
            name: "b".repeat(64),
            amount: 1,
            state: "PENDING".to_string(),
            puzzle_hash: String::new(),
        };
        assert!(wallet_coin_spendable(&confirmed));
        assert!(!wallet_coin_spendable(&pending));
    }

    #[tokio::test]
    async fn resolve_bootstrap_split_fee_uses_coinset_conservative_fee() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_fee_estimate")
            .with_status(200)
            .with_body(r#"{"success":true,"estimates":[100,500]}"#)
            .create_async()
            .await;

        let signer = test_signer_config(&server.url());

        let (fee_mojos, fee_source, lookup_error) =
            resolve_bootstrap_split_fee(&signer, "mainnet", 99, 2).await;
        assert_eq!(fee_mojos, 500);
        assert_eq!(fee_source, "coinset_conservative_fee");
        assert!(lookup_error.is_none());
    }

    #[tokio::test]
    async fn resolve_bootstrap_split_fee_falls_back_on_lookup_failure() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_fee_estimate")
            .with_status(500)
            .create_async()
            .await;

        let signer = test_signer_config(&server.url());

        let (fee_mojos, fee_source, lookup_error) =
            resolve_bootstrap_split_fee(&signer, "mainnet", 99, 2).await;
        assert_eq!(fee_mojos, 99);
        assert_eq!(fee_source, "config_minimum_fee_fallback");
        assert!(lookup_error.is_some());
    }

    #[tokio::test]
    async fn resolve_bootstrap_split_fee_falls_back_when_estimate_empty() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_fee_estimate")
            .with_status(200)
            .with_body(r#"{"success":false}"#)
            .create_async()
            .await;

        let signer = test_signer_config(&server.url());

        let (fee_mojos, fee_source, lookup_error) =
            resolve_bootstrap_split_fee(&signer, "mainnet", 99, 2).await;
        assert_eq!(fee_mojos, 99);
        assert_eq!(fee_source, "config_minimum_fee_fallback");
        assert!(lookup_error.is_none());
    }
}
