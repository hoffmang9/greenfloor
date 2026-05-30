//! GreenFloor Rust engine: vault KMS signing and deterministic daemon policy.
//!
//! The Rust crate and PyO3 module are named `greenfloor_engine` (ADR 0010).
//! Policy is grouped by domain (`cycle/`, `coin_ops/`, `offer/`, `vault/`).

#![recursion_limit = "1024"]

pub mod adapters;
pub mod coin_ops;
pub mod coinset;
pub mod config;
pub mod cycle;
pub mod daemon;
pub mod error;
pub mod hex;
pub mod kms;
pub mod manager;
pub mod offer;
pub mod storage;
pub mod vault;

use config::SignerConfig;
use error::SignerResult;

pub async fn resolve_vault_context(config: SignerConfig) -> SignerResult<vault::VaultContext> {
    Ok(vault::session::resolve_vault_session(config).await?.display)
}

pub async fn resolve_offer_assets_via_coinset(
    config: SignerConfig,
    base_asset: &str,
    quote_asset: &str,
) -> SignerResult<(String, String)> {
    offer::resolve_offer_assets_via_coinset(&config, base_asset, quote_asset).await
}

/// Deprecated alias for [`resolve_offer_assets_via_coinset`].
pub async fn resolve_offer_asset_ids(
    config: SignerConfig,
    base_asset: &str,
    quote_asset: &str,
) -> SignerResult<(String, String)> {
    resolve_offer_assets_via_coinset(config, base_asset, quote_asset).await
}

pub use coin_ops::{
    amount_meets_coin_op_min_mojos, coin_op_min_amount_mojos, coin_op_should_stop,
    coin_op_target_amount_allowed, compute_bucket_counts_from_coins,
    effective_sell_bucket_counts_for_coin_ops, evaluate_coin_combine_gate,
    evaluate_coin_split_gate, fee_budget_allows_execution, is_spendable_wallet_coin,
    partition_plans_by_budget, plan_auto_combine_inputs, plan_auto_split_selection, plan_coin_ops,
    projected_coin_ops_fee_mojos, select_spendable_coins_for_target_amount,
    split_would_create_sub_cat_change, BucketSpec, CoinCombineGateResult, CoinOpKind, CoinOpPlan,
    CoinSplitGateResult, CombineInputSelectionMode, SpendableCoin, SplitAutoSelectPlan,
    SplitCoinPlan, SplitCombinePrereqPlan, SplitPlanningProfile, SplitSkipPlan,
};
pub use coinset::{
    extract_coin_id_hints_from_offer_text, get_conservative_fee_estimate, get_fee_estimate,
    is_canonical_xch_asset, is_xch_like_asset, list_wallet_unspent_coins, parse_coin_ids,
    push_tx_hex, spend_bundle_hash_from_hex, WalletUnspentCoin,
};
pub use config::load_signer_config;
pub use config::{
    load_markets_config, load_markets_config_with_overlay, load_program_config,
    require_signer_offer_path, resolve_market_for_build, ManagerProgramConfig, MarketConfig,
    MarketsConfig,
};
pub use cycle::{
    abs_move_bps, aggregate_two_sided_offer_counts, apply_offer_signal,
    can_parallelize_managed_offers, cancel_move_threshold_bps, classify_dexie_stale_offer_status,
    classify_dexie_visibility_outcome, classify_managed_post_result,
    classify_managed_transient_error, coinset_fee_lookup_retry_sleep,
    collect_open_offer_ids_for_cancel, collect_stale_sweep_candidates,
    count_parallel_transient_failures, dedupe_sorted_market_ids, dexie_invalid_offer_retry_sleep,
    dexie_invalid_offer_should_retry, enqueue_immediate_requeue, evaluate_cancel_policy_decision,
    evaluate_low_inventory_alert, evaluate_market, evaluate_two_sided_market_actions,
    executed_sell_offer_counts_by_size, expand_planned_actions, expiry_seconds_for_action,
    filter_planned_actions_with_positive_repeat, is_dexie_offer_missing_error_text,
    is_managed_upstream_transient_error, is_managed_worker_transient_error,
    is_parallel_dispatch_transient_error, is_transient_dexie_visibility_404_error,
    is_transient_managed_upstream_error_text, is_two_sided_market_mode, managed_retry_decision,
    market_cycle_phases, moderate_retry_next_sleep, moderate_retry_sleep_seconds,
    needs_inventory_fallback, next_disabled_market_log_deadline, one_sided_offer_counts_by_side,
    parallel_max_workers, parse_rate_limit_retry_seconds, plan_parallel_managed_dispatch,
    plan_reseed_actions_from_gap, poll_exponential_advance_sleep, poll_exponential_sleep_now,
    record_stale_sweep_check, reseed_skip_reason_labels, reservation_release_status,
    resolve_inventory_scan_source, resolve_missing_watched_offer_transition, resolve_tracked_sizes,
    resolve_watched_offer_transition_from_signals, select_market_batch,
    should_apply_parallel_transient_cooldown, should_log_disabled_market,
    should_try_cat_inventory_fallback, should_use_market_slot_dispatch,
    single_input_preferred_skip_reason, unchanged_offer_transition,
    unsupported_venue_offer_transition, wallet_fallback_source_label, AlertEvent, AlertState,
    CancelPolicyDecision, CycleOfferTransition, LowInventoryEvaluation, LowInventoryInput,
    ManagedActionOutcome, ManagedActionStatus, ManagedRetryDecision, ManagedRetryDecisionKind,
    MarketBatchSelection, MarketCyclePhase, MarketCycleResultState, MarketState,
    OfferLifecycleState, OfferSignal, OfferStateRow, OfferTransition, ParallelBatchPlan,
    ParallelQueueItem, ParallelReservationContext, ParallelSkipItem, ParallelSubmissionDecision,
    PlannedAction, PlannedActionInput, ReseedGapPlan, ReseedSkipReason, SpendableAssetProfile,
    StaleSweepCandidate, StaleSweepHit, StaleSweepProgress, StrategyConfig,
};
pub use error::SignerError as Error;
pub use hex::{default_mojo_multiplier_for_asset, is_hex_id, normalize_hex_id};
pub use manager::{
    build_and_post_offer, format_build_and_post_output, BuildAndPostOfferRequest,
    BuildAndPostOfferResponse,
};
pub use daemon::{
    run_daemon_command, run_daemon_cycle_once_from_json, run_daemon_loop_from_json,
    run_offers_cancel_command, run_offers_reconcile_command, run_offers_status_command,
    DaemonCliArgs, OffersCancelCliArgs, OffersReconcileCliArgs, OffersStatusCliArgs,
};
pub use offer::bootstrap::{
    bootstrap_early_phase, bootstrap_executed_phase, plan_bootstrap_mixed_outputs, BootstrapCoin,
    BootstrapPhaseSnapshot, BootstrapPlan, BootstrapPlanOutcome, LadderDeficit, PlannerLadderRow,
};
pub use offer::build_context::{
    mojo_multiplier_for_leg, resolve_offer_expiry_for_pricing, resolve_quote_price_for_pricing,
};
pub use offer::codec::{
    encode_offer_from_spend_bundle_bytes, from_input_spend_bundle_bytes,
    from_input_spend_bundle_xch_bytes, validate_offer_structure, validate_offer_text,
    verify_offer_for_dexie,
};
pub use offer::publish::{
    bootstrap_block_error, dexie_offer_asset_expectation_error, expected_publish_asset_fields,
    ExpectedPublishAssetFields,
};
pub use offer::request::{
    compute_signer_offer_leg_amounts, normalize_offer_asset_id, normalize_offer_side,
    quote_mojos_for_base_size, signer_split_asset_id, SignerOfferLegAmounts,
};
pub use offer::{
    build_signer_offer_for_action, build_vault_cat_offer, expires_at_unix_from_pricing,
    resolve_offer_assets_for_action, try_normalize_resolved_assets, BuildOfferForActionRequest,
    BuildOfferForActionResult, CreateOfferRequest, CreateOfferResult,
};
pub use vault::{
    build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest, MixedSplitResult,
};

#[cfg(test)]
mod test_support;
