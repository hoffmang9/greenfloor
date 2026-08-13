use crate::coin_ops::is_spendable_coin_state;
use crate::coinset::{get_conservative_fee_estimate_for_signer, WalletUnspentCoin};
use crate::config::{LadderEntry, MarketPricing, SignerConfig};
use crate::error::{OfferError, SignerError, SignerResult};
use crate::offer::bootstrap::{BootstrapCoin, PlanAmount, PlannerLadderRow};
use crate::offer::build_context::mojo_multiplier_for_leg;
use crate::offer::pricing::quote_mojos_for_base_size;
use crate::offer::request::normalize_offer_side;

/// Convert one ladder clip to bootstrap plan mojos.
///
/// Sell: `size_base_units * base_mojo_multiplier`.
/// Buy: quote mojos for the base clip at `quote_price` (already mojos).
pub(super) fn bootstrap_size_mojos_for_side(
    side: &str,
    size_base_units: i64,
    pricing: &MarketPricing,
    quote_price: f64,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
) -> SignerResult<i64> {
    let side = normalize_offer_side(side);
    if side == "buy" {
        let quote_mult = mojo_multiplier_for_leg(
            pricing,
            "quote_unit_mojo_multiplier",
            resolved_quote_asset_id,
        );
        quote_mojos_for_base_size(size_base_units, quote_price, quote_mult)
    } else {
        let base_mult =
            mojo_multiplier_for_leg(pricing, "base_unit_mojo_multiplier", resolved_base_asset_id)
                .max(1);
        size_base_units
            .checked_mul(base_mult)
            .ok_or(SignerError::Offer(OfferError::InvalidOfferRequestAmount))
    }
}

/// Convert market ladder sizes into bootstrap plan mojos for both sides.
pub(super) fn bootstrap_ladder_entries_for_side(
    side: &str,
    side_ladder: &[LadderEntry],
    pricing: &MarketPricing,
    quote_price: f64,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
) -> SignerResult<Vec<PlannerLadderRow>> {
    let mut entries = Vec::new();
    for entry in side_ladder {
        let size_mojos = bootstrap_size_mojos_for_side(
            side,
            entry.size_base_units,
            pricing,
            quote_price,
            resolved_base_asset_id,
            resolved_quote_asset_id,
        )?;
        if size_mojos <= 0 {
            continue;
        }
        entries.push(PlannerLadderRow {
            size: size_mojos,
            target_count: entry.target_count,
            split_buffer_count: entry.split_buffer_count,
        });
    }
    Ok(entries)
}

/// Converted plan-mojo clip for the offer being posted, if that size is on the ladder.
pub(super) fn action_clip_mojos_for_side(
    side: &str,
    side_ladder: &[LadderEntry],
    action_size_base_units: u64,
    pricing: &MarketPricing,
    quote_price: f64,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
) -> SignerResult<Option<i64>> {
    let Some(action) = i64::try_from(action_size_base_units)
        .ok()
        .filter(|size| *size > 0)
    else {
        return Ok(None);
    };
    if !side_ladder
        .iter()
        .any(|entry| entry.size_base_units == action)
    {
        return Ok(None);
    }
    let mojos = bootstrap_size_mojos_for_side(
        side,
        action,
        pricing,
        quote_price,
        resolved_base_asset_id,
        resolved_quote_asset_id,
    )?;
    Ok((mojos > 0).then_some(mojos))
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

/// Map wallet coin mojos onto bootstrap plan amounts (also mojos).
pub(super) fn bootstrap_coins_as_plan_mojos(coins: &[WalletUnspentCoin]) -> Vec<BootstrapCoin> {
    coins
        .iter()
        .filter(|coin| wallet_coin_spendable(coin))
        .filter_map(|coin| {
            let amount_mojos = i64::try_from(coin.amount).ok()?;
            (amount_mojos > 0).then(|| BootstrapCoin {
                id: coin.id.clone(),
                amount: PlanAmount::new(amount_mojos),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        action_clip_mojos_for_side, bootstrap_coins_as_plan_mojos,
        bootstrap_ladder_entries_for_side, resolve_bootstrap_split_fee, wallet_coin_spendable,
    };
    use crate::coinset::WalletUnspentCoin;
    use crate::config::{LadderEntry, MarketPricing};
    use crate::test_support::signer_config::test_signer_config;

    #[test]
    fn bootstrap_ladder_entries_for_sell_side_converts_to_mojos() {
        let ladder = vec![LadderEntry {
            size_base_units: 25,
            target_count: 3,
            split_buffer_count: 1,
            combine_when_excess_factor: 2.0,
        }];
        let pricing = MarketPricing {
            base_unit_mojo_multiplier: Some(1_000),
            ..MarketPricing::default()
        };
        let entries = bootstrap_ladder_entries_for_side(
            "sell",
            &ladder,
            &pricing,
            1.0,
            "0000000000000000000000000000000000000000000000000000000000000001",
            "xch",
        )
        .expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size, 25_000);
        assert_eq!(entries[0].target_count, 3);
    }

    #[test]
    fn action_clip_mojos_for_side_converts_ladder_row() {
        let ladder = vec![
            LadderEntry {
                size_base_units: 10,
                target_count: 4,
                split_buffer_count: 1,
                combine_when_excess_factor: 2.0,
            },
            LadderEntry {
                size_base_units: 25,
                target_count: 1,
                split_buffer_count: 1,
                combine_when_excess_factor: 2.0,
            },
        ];
        let pricing = MarketPricing {
            base_unit_mojo_multiplier: Some(1_000),
            ..MarketPricing::default()
        };
        let base = "0000000000000000000000000000000000000000000000000000000000000001";
        assert_eq!(
            action_clip_mojos_for_side("sell", &ladder, 10, &pricing, 1.0, base, "xch")
                .expect("clip"),
            Some(10_000)
        );
        assert_eq!(
            action_clip_mojos_for_side("sell", &ladder, 25, &pricing, 1.0, base, "xch")
                .expect("clip"),
            Some(25_000)
        );
        assert_eq!(
            action_clip_mojos_for_side("sell", &ladder, 7, &pricing, 1.0, base, "xch")
                .expect("clip"),
            None
        );
        assert_eq!(
            action_clip_mojos_for_side("sell", &ladder, 0, &pricing, 1.0, base, "xch")
                .expect("clip"),
            None
        );
    }

    #[test]
    fn action_clip_mojos_for_side_drops_zero_mojo_row() {
        let ladder = vec![
            LadderEntry {
                size_base_units: 1,
                target_count: 1,
                split_buffer_count: 0,
                combine_when_excess_factor: 2.0,
            },
            LadderEntry {
                size_base_units: 10,
                target_count: 4,
                split_buffer_count: 1,
                combine_when_excess_factor: 2.0,
            },
        ];
        let pricing = MarketPricing {
            quote_unit_mojo_multiplier: Some(1_000),
            ..MarketPricing::default()
        };
        let quote = "0000000000000000000000000000000000000000000000000000000000000001";
        // 1 * 0.0004 * 1000 = 0.4 → 0 mojos; 10 * 0.0004 * 1000 = 4.
        assert_eq!(
            action_clip_mojos_for_side("buy", &ladder, 1, &pricing, 0.0004, "xch", quote)
                .expect("clip"),
            None
        );
        assert_eq!(
            action_clip_mojos_for_side("buy", &ladder, 10, &pricing, 0.0004, "xch", quote)
                .expect("clip"),
            Some(4)
        );
        let entries =
            bootstrap_ladder_entries_for_side("buy", &ladder, &pricing, 0.0004, "xch", quote)
                .expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size, 4);
    }

    #[test]
    fn bootstrap_ladder_entries_for_buy_side_converts_quote_sizes() {
        let ladder = vec![LadderEntry {
            size_base_units: 10,
            target_count: 2,
            split_buffer_count: 0,
            combine_when_excess_factor: 2.0,
        }];
        let pricing = MarketPricing {
            quote_unit_mojo_multiplier: Some(1000),
            ..MarketPricing::default()
        };
        let entries = bootstrap_ladder_entries_for_side(
            "buy",
            &ladder,
            &pricing,
            2.0,
            "xch",
            "0000000000000000000000000000000000000000000000000000000000000001",
        )
        .expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size, 20_000);
    }

    #[test]
    fn bootstrap_ladder_entries_for_buy_side_keeps_fractional_quote_clip_in_mojos() {
        let ladder = vec![LadderEntry {
            size_base_units: 10,
            target_count: 1,
            split_buffer_count: 1,
            combine_when_excess_factor: 2.0,
        }];
        let pricing = MarketPricing {
            quote_unit_mojo_multiplier: Some(1000),
            ..MarketPricing::default()
        };
        let entries = bootstrap_ladder_entries_for_side(
            "buy",
            &ladder,
            &pricing,
            1.001,
            "00000000000000000000000000000000000000000000000000000000000000aa",
            "00000000000000000000000000000000000000000000000000000000000000bb",
        )
        .expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size, 10_010);
    }

    #[test]
    fn bootstrap_coins_as_plan_mojos_keeps_raw_mojo_amounts() {
        use crate::offer::bootstrap::PlanAmount;

        let coins = vec![
            WalletUnspentCoin {
                id: "a".repeat(64),
                name: "a".repeat(64),
                amount: 10_010,
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
        let plan_coins = bootstrap_coins_as_plan_mojos(&coins);
        assert_eq!(plan_coins.len(), 2);
        assert_eq!(plan_coins[0].amount, PlanAmount::new(10_010));
        assert_eq!(plan_coins[1].amount, PlanAmount::new(500));
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
