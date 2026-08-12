//! Managed offer posts for daemon strategy dispatch.
//!
//! Two entry points share [`execute_managed_post`]:
//! - [`post_managed_planned_action`] — borrowed [`ManagedPostContext`] for sequential dispatch.
//! - [`post_managed_planned_action_owned`] — `Arc<ManagedPostContext>` plus owned market/action
//!   for `tokio::spawn` workers that require `'static` futures.

use std::sync::Arc;

use crate::config::{CatTickerIndex, ManagerProgramConfig, MarketConfig, SignerConfig};
use crate::cycle::PlannedAction;
use crate::daemon::cycle_paths::DaemonCyclePaths;
use crate::daemon::market_context::MarketCycleContext;
use crate::error::SignerResult;
use crate::offer::operator::{ensure_size_n_offer, BuildAndPostOfferRequestParts};
use crate::offer::request::normalize_offer_side;
use crate::storage::CycleWriteStore;

use crate::async_boundary::{ManagedOfferPostFuture, OwnedManagedOfferPostFuture};

#[cfg(test)]
use crate::daemon::dispatch_test_controls::DaemonDispatchTestInjections;

/// Owned dispatch inputs for managed offer posts (sequential and parallel workers).
#[derive(Debug, Clone)]
pub(super) struct ManagedPostContext {
    pub program: ManagerProgramConfig,
    pub signer: SignerConfig,
    pub ticker_index: CatTickerIndex,
    pub operator_network: String,
    pub paths: DaemonCyclePaths,
    pub write_store: CycleWriteStore,
    #[cfg(test)]
    pub dispatch_injections: DaemonDispatchTestInjections,
}

impl ManagedPostContext {
    pub(super) fn from_market_cycle(ctx: &MarketCycleContext<'_>) -> SignerResult<Self> {
        Ok(Self {
            program: ctx.resources.program().clone(),
            signer: ctx.resources.signer_for_execution()?.clone(),
            ticker_index: ctx.resources.ticker_index.clone(),
            operator_network: ctx.resources.network.clone(),
            paths: ctx.resources.paths.clone(),
            write_store: ctx.dispatch.write_store.clone(),
            #[cfg(test)]
            dispatch_injections: ctx.dispatch.test_controls.offer_dispatch.clone(),
        })
    }
}

fn daemon_ensure_parts(
    post_ctx: &ManagedPostContext,
    market: &MarketConfig,
    action: &PlannedAction,
) -> SignerResult<BuildAndPostOfferRequestParts> {
    let size_base_units = crate::config::parse_non_negative_u64(action.size, "action.size")?;
    Ok(BuildAndPostOfferRequestParts::for_ensure_size(
        &post_ctx.paths.as_operator_paths(),
        &post_ctx.program,
        post_ctx.operator_network.clone(),
        market.market_id.clone(),
        size_base_units,
        normalize_offer_side(&action.side),
    ))
}

async fn execute_managed_post(
    post_ctx: &ManagedPostContext,
    market: &MarketConfig,
    action: &PlannedAction,
) -> SignerResult<bool> {
    #[cfg(test)]
    if let Some(result) =
        super::test_overrides::managed_post_result(post_ctx, &post_ctx.dispatch_injections)
    {
        return result;
    }
    if action.size <= 0 {
        return Ok(false);
    }
    let parts = daemon_ensure_parts(post_ctx, market, action)?;
    ensure_size_n_offer(
        &post_ctx.write_store,
        post_ctx.signer.clone(),
        &post_ctx.ticker_index,
        market,
        parts,
    )
    .await
}

pub fn post_managed_planned_action<'a>(
    post_ctx: &'a ManagedPostContext,
    market: &'a MarketConfig,
    action: &'a PlannedAction,
) -> ManagedOfferPostFuture<'a> {
    Box::pin(execute_managed_post(post_ctx, market, action))
}

pub fn post_managed_planned_action_owned(
    post_ctx: Arc<ManagedPostContext>,
    market: MarketConfig,
    action: PlannedAction,
) -> OwnedManagedOfferPostFuture {
    Box::pin(async move { execute_managed_post(&post_ctx, &market, &action).await })
}

#[cfg(test)]
pub(super) fn flush_managed_post_persist_for_test(
    post_ctx: &ManagedPostContext,
) -> SignerResult<()> {
    use crate::offer::operator::{empty_persist_artifacts_for_test, flush_build_and_post_persist};

    let store = post_ctx.write_store.lock()?;
    flush_build_and_post_persist(&store, &empty_persist_artifacts_for_test())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::{CatTickerIndex, ManagerProgramConfig};
    use crate::cycle::PlannedAction;
    use crate::daemon::cycle_paths::DaemonCyclePaths;
    use crate::daemon::dispatch_test_controls::DaemonDispatchTestInjections;
    use crate::storage::CycleWriteStore;
    use crate::test_support::market_config::sample_market;
    use crate::test_support::signer_config::test_signer_config;

    fn sample_post_context(store: CycleWriteStore) -> ManagedPostContext {
        ManagedPostContext {
            program: ManagerProgramConfig {
                network: "mainnet".to_string(),
                runtime_dry_run: false,
                offer_publish_venue: "dexie".to_string(),
                dexie_api_base: "https://dexie.example".to_string(),
                splash_api_base: "https://splash.example".to_string(),
                ..Default::default()
            },
            signer: test_signer_config("https://api.coinset.org"),
            ticker_index: CatTickerIndex::default(),
            operator_network: "mainnet".to_string(),
            paths: DaemonCyclePaths::new(
                PathBuf::from("/tmp/program.yaml"),
                PathBuf::from("/tmp/markets.yaml"),
                None,
            ),
            write_store: store,
            dispatch_injections: DaemonDispatchTestInjections::default(),
        }
    }

    fn sample_action(size: i64) -> PlannedAction {
        PlannedAction {
            size,
            repeat: 1,
            side: "sell".to_string(),
            pair: String::new(),
            expiry_unit: "minutes".to_string(),
            expiry_value: 10,
            cancel_after_create: false,
            reason: "test".to_string(),
        }
    }

    #[test]
    fn daemon_ensure_parts_builds_size_and_side() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CycleWriteStore::open(&dir.path().join("state.db")).expect("open");
        let post_ctx = sample_post_context(store);
        let market = sample_market("xch1test");
        let action = sample_action(25);

        let parts = daemon_ensure_parts(&post_ctx, &market, &action).expect("ensure parts");

        assert_eq!(parts.network, "mainnet");
        assert_eq!(parts.market_id.as_deref(), Some("m1"));
        assert_eq!(parts.size_base_units, 25);
        assert_eq!(parts.action_side.as_deref(), Some("sell"));
        assert_eq!(parts.publish_venue.as_deref(), Some("dexie"));
        assert!(parts.venue.drop_only);
        assert_eq!(parts.repeat, 1);
    }

    #[tokio::test]
    async fn execute_managed_post_skips_non_positive_size_without_network() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CycleWriteStore::open(&dir.path().join("state.db")).expect("open");
        let post_ctx = sample_post_context(store);
        let market = sample_market("xch1test");
        let action = sample_action(0);

        let posted = post_managed_planned_action(&post_ctx, &market, &action)
            .await
            .expect("managed post");
        assert!(!posted);
    }
}
