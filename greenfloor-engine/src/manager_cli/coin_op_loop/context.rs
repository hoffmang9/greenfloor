use std::collections::HashSet;

use serde_json::{json, Value};

use crate::coin_ops::execution::CoinOpExecContext;
#[cfg(test)]
use crate::coin_ops::execution::CoinOpTestOverrides;
use crate::coin_ops::SpendableCoin;
use crate::config::{load_gated_operator_market, GatedOperatorMarketLoadRequest};
use crate::error::SignerResult;
use crate::offer::OfferAssetResolver;
use crate::storage::{resolve_state_db_path, SqliteStore};

pub(super) const COIN_SPLIT_LOCKUP_ERROR: &str =
    "coin_split_lockup_guardrail_would_lock_all_spendable_coins";
pub(super) const COIN_SPLIT_NO_SPENDABLE_ERROR: &str = "no_spendable_split_coin_available";

/// Load durable maker coin-id watches for CLI coin-ops (same set as daemon).
///
/// Exclusion is applied inside [`CoinOpExecContext::list_spendable_coins`], which
/// both the until-ready loop and daemon runners use for selection.
fn load_market_watched_coin_ids(
    home_dir: &std::path::Path,
    state_db_override: Option<&str>,
    market_id: &str,
) -> SignerResult<HashSet<String>> {
    let db_path = resolve_state_db_path(home_dir, state_db_override);
    if !db_path.exists() {
        return Ok(HashSet::default());
    }
    let store = SqliteStore::open(&db_path)?;
    store.list_watched_coin_ids_for_market(market_id)
}

pub(super) async fn build_coin_op_exec_context(
    request: &GatedOperatorMarketLoadRequest<'_>,
    asset_id_override: Option<&str>,
    state_db_override: Option<&str>,
) -> SignerResult<CoinOpExecContext> {
    let gated = load_gated_operator_market(request)?;
    let watched_coin_ids = load_market_watched_coin_ids(
        &gated.program.home_dir,
        state_db_override,
        &gated.market_row.market_id,
    )?;
    CoinOpExecContext::from_gated_market(
        gated,
        asset_id_override,
        watched_coin_ids,
        #[cfg(test)]
        CoinOpTestOverrides::default(),
    )
    .await
}

pub(super) fn enforce_split_lockup_guardrail(
    spendable: &[SpendableCoin],
    selected_coin_ids: &[String],
    allow_lock_all_spendable: bool,
    resolved_asset_id: &str,
) -> Option<(i32, Value)> {
    if allow_lock_all_spendable {
        return None;
    }
    let spendable_ids: HashSet<_> = spendable.iter().map(|coin| coin.id.clone()).collect();
    let selected_set: HashSet<String> = selected_coin_ids.iter().cloned().collect();
    if spendable_ids.is_empty() || selected_set != spendable_ids {
        return None;
    }
    Some((
        2,
        json!({
            "error": COIN_SPLIT_LOCKUP_ERROR,
            "resolved_asset_id": resolved_asset_id,
            "spendable_asset_coin_count": spendable_ids.len(),
            "selected_spendable_coin_count": selected_set.len(),
        }),
    ))
}

pub(super) fn spendable_coins_for_gate(spendable: &[SpendableCoin]) -> Vec<Value> {
    spendable
        .iter()
        .map(|coin| {
            json!({
                "amount": coin.amount,
                "state": "CONFIRMED",
            })
        })
        .collect()
}

pub(super) async fn resolve_asset_filter(
    resolver: &OfferAssetResolver<'_>,
    filter: &str,
) -> SignerResult<String> {
    resolver.resolve_inventory_asset(filter).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_market_watched_coin_ids_reads_durable_watches_when_db_exists() {
        let dir = tempdir().expect("tempdir");
        let home = dir.path();
        let db_path = resolve_state_db_path(home, None);
        std::fs::create_dir_all(db_path.parent().expect("parent")).expect("mkdir");
        let store = SqliteStore::open(&db_path).expect("open");
        let coin = "ab".repeat(32);
        let p2 = "cd".repeat(32);
        store
            .ensure_offer_coin_watches(
                "offer1",
                "m1",
                std::slice::from_ref(&coin),
                std::slice::from_ref(&p2),
            )
            .expect("ensure");
        let coins = load_market_watched_coin_ids(home, None, "m1").expect("load");
        assert!(coins.contains(&coin));
        // Coin-ops ignores p2 watches; loader must not require them.
        assert!(!coins.contains(&p2));
    }

    #[test]
    fn load_market_watched_coin_ids_empty_when_db_missing() {
        let dir = tempdir().expect("tempdir");
        let coins = load_market_watched_coin_ids(dir.path(), None, "m1").expect("missing db ok");
        assert!(coins.is_empty());
    }
}
