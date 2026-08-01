//! Soft-expire open listings and ensure/reclaim durable maker coins after expiry.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{info, warn};

use crate::config::{
    bake_expiry_into_conditions_for_quote, market_wants_ladder_size, MarketConfig,
};
use crate::cycle::OfferLifecycleState;
use crate::daemon::market_context::MarketCycleContext;
use crate::error::SignerResult;
use crate::offer::operator::{
    ensure_size_n_offer, reclaim_expired_maker_if_unspent, EnsureSizeConfigPaths,
    EnsureSizeNOfferRequest,
};
use crate::offer::request::normalize_offer_side;
use crate::storage::{CycleWriteStore, SqliteStore};

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn mark_soft_expired(store: &SqliteStore, market_id: &str, now: i64) -> SignerResult<usize> {
    let rows = store.list_open_offers_past_listing_expiry(market_id, now)?;
    let mut marked = 0usize;
    for row in rows {
        store.upsert_offer_state(
            &row.offer_id,
            market_id,
            OfferLifecycleState::Expired.as_str(),
            Some(6),
        )?;
        marked = marked.saturating_add(1);
        info!(
            offer_id = %row.offer_id,
            market_id,
            "soft listing expiry → expired"
        );
    }
    Ok(marked)
}

/// Soft-expire listings (stable) then ensure size-N offer or reclaim expired makers.
///
/// For a wanted size N, call [`ensure_size_n_offer`] once; surplus expired makers for that
/// size are reclaimed (never left stranded).
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
            let _ = mark_soft_expired(store, &market_id, now)?;
        }
        store.list_expired_presplit_makers(&market_id)
    })?;
    if expired.is_empty() {
        return Ok(());
    }

    let signer = ctx.resources.signer_for_execution()?.clone();
    let program = ctx.resources.program();
    let mut ensured_sizes: HashSet<(String, i64)> = HashSet::new();

    for row in expired {
        let side = normalize_offer_side(row.offer_side.as_deref().unwrap_or("sell")).to_string();
        let Some(ladder_size) = row.size_base_units.filter(|units| *units > 0) else {
            warn!(offer_id = %row.offer_id, "expired maker missing size_base_units; reclaim");
            let _ = reclaim_expired_maker_if_unspent(
                write_store,
                signer.clone(),
                &ctx.resources.network,
                &row,
                program.runtime_dry_run,
            )
            .await?;
            continue;
        };

        let want = market_wants_ladder_size(market, &side, ladder_size);
        let key = (side.clone(), ladder_size);
        // Wanted size: ensure once. Unwanted or surplus expired makers: reclaim.
        if !want || !ensured_sizes.insert(key) {
            let outcome = reclaim_expired_maker_if_unspent(
                write_store,
                signer.clone(),
                &ctx.resources.network,
                &row,
                program.runtime_dry_run,
            )
            .await?;
            info!(
                offer_id = %row.offer_id,
                ladder_size,
                want,
                ?outcome,
                "expired maker reclaimed"
            );
            continue;
        }

        let size_u64 = crate::config::parse_non_negative_u64(ladder_size, "size_base_units")?;
        let ensure_request = EnsureSizeNOfferRequest::from_program(
            EnsureSizeConfigPaths {
                program_path: ctx.resources.paths.program_path.clone(),
                markets_path: ctx.resources.paths.markets_path.clone(),
                testnet_markets_path: ctx.resources.paths.testnet_markets_path.clone(),
            },
            program,
            ctx.resources.network.clone(),
            market.clone(),
            size_u64,
            side,
        );
        let outcome = ensure_size_n_offer(
            write_store,
            signer.clone(),
            &ctx.resources.ticker_index,
            ensure_request,
        )
        .await?;
        info!(
            offer_id = %row.offer_id,
            ladder_size,
            ?outcome,
            "ensure_size_n_offer after expire"
        );
    }
    Ok(())
}
