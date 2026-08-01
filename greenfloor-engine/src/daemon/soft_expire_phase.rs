//! Soft-expire open listings and ensure/reclaim durable maker coins after expiry.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{info, warn};

use crate::config::{
    bake_expiry_into_conditions_for_quote, market_wants_ladder_size, MarketConfig,
};
use crate::daemon::market_context::MarketCycleContext;
use crate::error::SignerResult;
use crate::offer::lifecycle::mark_listings_soft_expired;
use crate::offer::operator::{
    ensure_size_n_offer, reclaim_expired_maker_if_unspent, BuildAndPostOfferRequestParts,
    EnsureSizeResult, OperatorConfigPaths,
};
use crate::offer::request::normalize_offer_side;
use crate::storage::{CycleWriteStore, ReusablePresplitMakerRow};

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SoftExpireSizeGroup {
    pub side: String,
    pub size_base_units: i64,
    pub makers: Vec<ReusablePresplitMakerRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SoftExpirePlanItem {
    EnsureGroup(SoftExpireSizeGroup),
    Reclaim(ReusablePresplitMakerRow),
}

/// Group expired makers by `(side, size)`: ensure once per wanted key, reclaim the rest.
#[must_use]
pub(crate) fn plan_soft_expire_work(
    expired: Vec<ReusablePresplitMakerRow>,
    market: &MarketConfig,
) -> Vec<SoftExpirePlanItem> {
    let mut order: Vec<(String, i64)> = Vec::new();
    let mut groups: HashMap<(String, i64), Vec<ReusablePresplitMakerRow>> = HashMap::new();
    let mut missing_size = Vec::new();

    for row in expired {
        let side = normalize_offer_side(row.offer_side.as_deref().unwrap_or("sell")).to_string();
        let Some(ladder_size) = row.size_base_units.filter(|units| *units > 0) else {
            missing_size.push(row);
            continue;
        };
        let key = (side, ladder_size);
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(row);
    }

    let mut plan = Vec::with_capacity(missing_size.len().saturating_add(order.len()));
    for row in missing_size {
        plan.push(SoftExpirePlanItem::Reclaim(row));
    }
    for key in order {
        let makers = groups.remove(&key).unwrap_or_default();
        if market_wants_ladder_size(market, &key.0, key.1) {
            plan.push(SoftExpirePlanItem::EnsureGroup(SoftExpireSizeGroup {
                side: key.0,
                size_base_units: key.1,
                makers,
            }));
        } else {
            for row in makers {
                plan.push(SoftExpirePlanItem::Reclaim(row));
            }
        }
    }
    plan
}

async fn reclaim_surplus_makers(
    write_store: &CycleWriteStore,
    ctx: &MarketCycleContext<'_>,
    makers: &[ReusablePresplitMakerRow],
    ensure_result: &EnsureSizeResult,
    dry_run: bool,
) -> SignerResult<()> {
    let signer = ctx.resources.signer_for_execution()?.clone();
    for row in makers {
        if ensure_result.protects_maker(row) {
            continue;
        }
        let outcome = reclaim_expired_maker_if_unspent(
            write_store,
            signer.clone(),
            &ctx.resources.network,
            row,
            dry_run,
        )
        .await?;
        info!(
            offer_id = %row.offer_id,
            outcome = ?outcome.outcome,
            "expired surplus maker reclaimed"
        );
    }
    Ok(())
}

/// Soft-expire listings (stable) then ensure size-N offer or reclaim expired makers.
///
/// Wanted sizes are ensured once per `(side, size)` group; surplus makers in that group
/// are reclaimed except the maker retained/superseded by ensure.
///
/// # Errors
///
/// Returns an error when store or ensure/reclaim paths fail hard.
pub async fn run_soft_expire_phase(
    write_store: &CycleWriteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
) -> SignerResult<()> {
    let now = now_unix();
    let market_id = market.market_id.clone();
    let soft = !bake_expiry_into_conditions_for_quote(&market.quote_asset_type);
    let expired = write_store.sync(|store| {
        if soft {
            let _ = mark_listings_soft_expired(store, &market_id, now)?;
        }
        store.list_expired_presplit_makers(&market_id)
    })?;
    if expired.is_empty() {
        return Ok(());
    }

    let signer = ctx.resources.signer_for_execution()?.clone();
    let program = ctx.resources.program();
    let dry_run = program.runtime_dry_run;
    let plan = plan_soft_expire_work(expired, market);

    for item in plan {
        match item {
            SoftExpirePlanItem::Reclaim(row) => {
                if row.size_base_units.is_none_or(|units| units <= 0) {
                    warn!(offer_id = %row.offer_id, "expired maker missing size_base_units; reclaim");
                }
                let outcome = reclaim_expired_maker_if_unspent(
                    write_store,
                    signer.clone(),
                    &ctx.resources.network,
                    &row,
                    dry_run,
                )
                .await?;
                info!(
                    offer_id = %row.offer_id,
                    outcome = ?outcome.outcome,
                    "expired maker reclaimed"
                );
            }
            SoftExpirePlanItem::EnsureGroup(group) => {
                let size_u64 = crate::config::parse_non_negative_u64(
                    group.size_base_units,
                    "size_base_units",
                )?;
                let paths = OperatorConfigPaths {
                    program_path: ctx.resources.paths.program_path.clone(),
                    markets_path: ctx.resources.paths.markets_path.clone(),
                    testnet_markets_path: ctx.resources.paths.testnet_markets_path.clone(),
                };
                let parts = BuildAndPostOfferRequestParts::for_ensure_size(
                    &paths,
                    program,
                    ctx.resources.network.clone(),
                    market.market_id.clone(),
                    size_u64,
                    &group.side,
                );
                let result = ensure_size_n_offer(
                    write_store,
                    signer.clone(),
                    &ctx.resources.ticker_index,
                    market,
                    parts,
                    Some(group.makers.clone()),
                )
                .await?;
                info!(
                    market_id = %market.market_id,
                    side = %group.side,
                    ladder_size = group.size_base_units,
                    outcome = ?result.outcome,
                    "ensure_size_n_offer after expire"
                );
                reclaim_surplus_makers(write_store, ctx, &group.makers, &result, dry_run).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::config::LadderEntry;
    use crate::test_support::market_config::sample_market;

    fn maker(
        offer_id: &str,
        size_base_units: Option<i64>,
        offer_side: &str,
    ) -> ReusablePresplitMakerRow {
        ReusablePresplitMakerRow {
            offer_id: offer_id.into(),
            market_id: "m1".into(),
            state: "expired".into(),
            size_base_units,
            offer_side: Some(offer_side.into()),
            cancel_input_coin_id: format!("coin-{offer_id}"),
            fixed_delegated_puzzle_hash: "hh".into(),
            offer_nonce: Some("nn".into()),
            listing_expires_at: None,
        }
    }

    #[test]
    fn plan_groups_wanted_size_and_reclaims_unwanted() {
        let mut market = sample_market("xch1test");
        market.quote_asset_type = "stable".into();
        let mut ladders = HashMap::new();
        ladders.insert(
            "sell".to_string(),
            vec![LadderEntry {
                size_base_units: 10,
                target_count: 1,
                split_buffer_count: 0,
                combine_when_excess_factor: 0.0,
            }],
        );
        market.ladders = ladders;

        let plan = plan_soft_expire_work(
            vec![
                maker("a", Some(10), "sell"),
                maker("b", Some(10), "sell"),
                maker("c", Some(99), "sell"),
                maker("d", None, "sell"),
            ],
            &market,
        );

        assert!(matches!(
            &plan[0],
            SoftExpirePlanItem::Reclaim(row) if row.offer_id == "d"
        ));
        match &plan[1] {
            SoftExpirePlanItem::EnsureGroup(group) => {
                assert_eq!(group.size_base_units, 10);
                assert_eq!(group.makers.len(), 2);
                assert_eq!(group.makers[0].offer_id, "a");
                assert_eq!(group.makers[1].offer_id, "b");
            }
            SoftExpirePlanItem::Reclaim(row) => {
                panic!("expected ensure group, got reclaim {}", row.offer_id)
            }
        }
        assert!(matches!(
            &plan[2],
            SoftExpirePlanItem::Reclaim(row) if row.offer_id == "c"
        ));
    }
}
