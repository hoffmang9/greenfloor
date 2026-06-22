use serde_json::{json, Value};

use crate::coin_ops::coin_op_target_amount_allowed;
use crate::config::{LadderEntry, MarketConfig};
use crate::error::SignerResult;
use crate::hex::default_mojo_multiplier_for_asset;
use crate::operator_log::{LogContext, COIN_OPS_SKIP_SUB_MINIMUM_TARGET_AMOUNT};
use crate::storage::SqliteStore;
use tracing::Level;

/// Classify sell-ladder rows into valid coin-op targets and sub-minimum rejects.
///
/// `base_unit_multiplier` is passed explicitly so tests can exercise sub-minimum paths
/// without depending on production multiplier defaults.
#[must_use]
pub(super) fn classify_sell_ladder_entries(
    base_asset: &str,
    base_unit_multiplier: i64,
    sell_ladder: &[LadderEntry],
) -> (Vec<LadderEntry>, Vec<Value>) {
    let base_asset = base_asset.trim();
    let mut valid_ladder = Vec::new();
    let mut invalid_buckets = Vec::new();
    for entry in sell_ladder {
        if entry.size_base_units <= 0 {
            continue;
        }
        let target_amount_mojos = entry.size_base_units.saturating_mul(base_unit_multiplier);
        if coin_op_target_amount_allowed(target_amount_mojos, base_asset) {
            valid_ladder.push(entry.clone());
            continue;
        }
        invalid_buckets.push(json!({
            "size_base_units": entry.size_base_units,
            "target_amount_mojos": target_amount_mojos,
        }));
    }
    (valid_ladder, invalid_buckets)
}

pub(super) fn record_sub_minimum_sell_ladder_skips(
    store: &SqliteStore,
    market: &MarketConfig,
    invalid_buckets: &[Value],
) -> SignerResult<()> {
    if invalid_buckets.is_empty() {
        return Ok(());
    }
    LogContext::MARKET_CYCLE.dual_audit(
        store,
        Level::WARN,
        "coin ops skipped sub-minimum target amount",
        COIN_OPS_SKIP_SUB_MINIMUM_TARGET_AMOUNT,
        &json!({
            "market_id": market.market_id,
            "invalid_bucket_count": invalid_buckets.len(),
            "invalid_buckets": invalid_buckets,
        }),
        Some(&market.market_id),
    )?;
    Ok(())
}

pub(super) fn build_valid_sell_ladder(
    store: &SqliteStore,
    market: &MarketConfig,
    sell_ladder: &[LadderEntry],
) -> SignerResult<Vec<LadderEntry>> {
    // Production CAT ladders (1 unit × 1000 mojos = minimum) never hit sub-minimum audit;
    // classify tests pass an explicit multiplier to exercise the reject path.
    let base_unit_multiplier = default_mojo_multiplier_for_asset(market.base_asset.trim());
    let (valid_ladder, invalid_buckets) =
        classify_sell_ladder_entries(market.base_asset.trim(), base_unit_multiplier, sell_ladder);
    record_sub_minimum_sell_ladder_skips(store, market, &invalid_buckets)?;
    Ok(valid_ladder)
}
