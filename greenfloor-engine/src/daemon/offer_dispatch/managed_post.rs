//! Managed offer posts for daemon strategy dispatch.
//!
//! Two entry points share [`execute_managed_post`]:
//! - [`post_managed_planned_action`] — borrowed [`ManagedPostContext`] for sequential dispatch.
//! - [`post_managed_planned_action_owned`] — `Arc<ManagedPostContext>` plus owned market/action
//!   for `tokio::spawn` workers that require `'static` futures.

use std::sync::Arc;

use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::cycle::PlannedAction;
use crate::daemon::cycle_paths::DaemonCyclePaths;
use crate::daemon::market_context::MarketCycleContext;
use crate::error::SignerResult;
use crate::offer::operator::{
    build_and_post_offer_with_persist_artifacts, flush_build_and_post_persist,
    BuildAndPostOfferRequestParts, BuildAndPostRunOptions, BuildAndPostVenueOptions,
};
use crate::offer::request::normalize_offer_side;
use crate::paths::resolve_cats_config_path;
use crate::storage::CycleWriteStore;

use crate::async_boundary::{ManagedOfferPostFuture, OwnedManagedOfferPostFuture};

#[cfg(test)]
use crate::daemon::dispatch_test_controls::DaemonDispatchTestInjections;

/// Owned dispatch inputs for managed offer posts (sequential and parallel workers).
#[derive(Debug, Clone)]
pub(super) struct ManagedPostContext {
    pub program: ManagerProgramConfig,
    pub paths: DaemonCyclePaths,
    pub write_store: CycleWriteStore,
    #[cfg(test)]
    pub dispatch_injections: DaemonDispatchTestInjections,
}

impl ManagedPostContext {
    pub(super) fn from_market_cycle(ctx: &MarketCycleContext<'_>) -> Self {
        Self {
            program: ctx.resources.program().clone(),
            paths: ctx.resources.paths.clone(),
            write_store: ctx.dispatch.write_store.clone(),
            #[cfg(test)]
            dispatch_injections: ctx.dispatch.test_controls.offer_dispatch.clone(),
        }
    }
}

fn daemon_managed_post_request(
    post_ctx: &ManagedPostContext,
    market: &MarketConfig,
    action: &PlannedAction,
) -> SignerResult<crate::offer::operator::BuildAndPostOfferRequest> {
    Ok(
        crate::offer::operator::BuildAndPostOfferRequest::from_parts(
            BuildAndPostOfferRequestParts {
                program_path: post_ctx.paths.program_path.clone(),
                markets_path: post_ctx.paths.markets_path.clone(),
                testnet_markets_path: post_ctx.paths.testnet_markets_path.clone(),
                cats_path: Some(resolve_cats_config_path(&post_ctx.paths.markets_path, None)),
                network: post_ctx.program.network.clone(),
                market_id: Some(market.market_id.clone()),
                pair: None,
                size_base_units: crate::config::parse_non_negative_u64(action.size, "action.size")?,
                repeat: 1,
                publish_venue: Some(post_ctx.program.offer_publish_venue.clone()),
                dexie_base_url: Some(post_ctx.program.dexie_api_base.clone()),
                splash_base_url: Some(post_ctx.program.splash_api_base.clone()),
                venue: BuildAndPostVenueOptions {
                    drop_only: true,
                    claim_rewards: false,
                },
                run: BuildAndPostRunOptions {
                    dry_run: post_ctx.program.runtime_dry_run,
                    persist_results: true,
                },
                action_side: Some(normalize_offer_side(&action.side).to_string()),
            },
        ),
    )
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
    let request = daemon_managed_post_request(post_ctx, market, action)?;
    let (response, artifacts) = build_and_post_offer_with_persist_artifacts(request).await?;
    if let Some(artifacts) = artifacts {
        let store = post_ctx.write_store.lock()?;
        flush_build_and_post_persist(&store, &artifacts)?;
    }
    Ok(response.exit_code == 0)
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
    use crate::offer::operator::empty_persist_artifacts_for_test;

    let store = post_ctx.write_store.lock()?;
    flush_build_and_post_persist(&store, &empty_persist_artifacts_for_test())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::ManagerProgramConfig;
    use crate::cycle::PlannedAction;
    use crate::daemon::cycle_paths::DaemonCyclePaths;
    use crate::daemon::dispatch_test_controls::DaemonDispatchTestInjections;
    use crate::storage::CycleWriteStore;
    use crate::test_support::market_config::sample_market;

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
            target_spread_bps: None,
        }
    }

    #[test]
    fn daemon_managed_post_request_builds_drop_only_offer_parts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CycleWriteStore::open(&dir.path().join("state.db")).expect("open");
        let post_ctx = sample_post_context(store);
        let market = sample_market("xch1test");
        let action = sample_action(25);

        let request =
            daemon_managed_post_request(&post_ctx, &market, &action).expect("managed post request");

        assert_eq!(request.network, "mainnet");
        assert_eq!(request.market_id.as_deref(), Some("m1"));
        assert_eq!(request.size_base_units, 25);
        assert_eq!(request.action_side.as_deref(), Some("sell"));
        assert!(request.venue.drop_only);
        assert!(!request.venue.claim_rewards);
        assert_eq!(request.publish_venue.as_deref(), Some("dexie"));
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
