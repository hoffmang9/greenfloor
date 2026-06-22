//! Canonical test-injection pattern for in-process operator tests.
//!
//! `GreenFloor` uses three independent injection bags (keep separate; do not merge):
//!
//! | Type | Layer | Carrier | Test entry |
//! |------|-------|---------|------------|
//! | [`CoinOpTestOverrides`](crate::coin_ops::execution::CoinOpTestOverrides) | coin ops | `CoinOpExecContext.test_overrides` | `run_coin_split_with_test_overrides`, `execute_managed_coin_op_plans_with_test_overrides` |
//! | [`BuildOfferTestOverrides`](crate::offer::operator::BuildOfferTestOverrides) | offer operator | `BuildAndPostOfferRequest.test_overrides` | `run_command_with_test_overrides` |
//! | [`DaemonDispatchTestInjections`](crate::daemon::dispatch_test_controls::DaemonDispatchTestInjections) | daemon dispatch | `DaemonCycleTestControls.offer_dispatch` | `ParallelDispatchHarness::set_offer_dispatch` |
//! | [`DaemonLoopTestHarness`](crate::daemon::loop_harness::DaemonLoopTestHarness) | daemon loop | in-process harness only | `run_daemon_loop_with_harness` |
//!
//! ## Rules
//!
//! - Override fields are `#[cfg(test)]` and, when serde applies, `#[serde(default, skip)]`.
//! - Read injections at subsystem boundaries and early-return before IO, signing, or coinset calls.
//! - Do not thread `#[cfg(test)]` parameters through intermediate call chains; carry on a context
//!   struct (`ManagedPostContext`, `ResolvedBuildAndPostContext`, `CoinOpExecContext`) instead.
//! - Unit tests cover injection branch tables; harness tests assert outcomes only (exit counts,
//!   audit events), not internal wiring.
//! - Env-gated CLI test controls (`GREENFLOOR_DAEMON_TEST_CONTROLS`) apply to serde-visible fields
//!   only; in-process dispatch injections bypass that gate by design.
