//! Typed async boundaries for public operator APIs.
//!
//! Box once at module entrypoints that return `impl Future` across crate boundaries.
//! Internal call chains use plain `async fn` (no nested `Box::pin`).

use std::future::Future;
use std::pin::Pin;

use crate::daemon::{CoinOpExecutionResult, OfferDispatchOutput};
use crate::error::SignerResult;
use crate::offer::operator::{BootstrapPhaseResult, BuildAndPostOfferResponse};
use crate::offer::types::CreateOfferResult;

/// Boxed future for the signer denomination bootstrap phase.
///
/// Boxed here because the async state machine exceeds Clippy's large-futures threshold
/// once ladder planning and vault split submission are composed.
pub type SignerDenominationPhaseFuture<'a> =
    Pin<Box<dyn Future<Output = SignerResult<BootstrapPhaseResult>> + Send + 'a>>;

/// Boxed future for managed offer build/post (daemon + CLI).
pub type BuildAndPostOfferFuture =
    Pin<Box<dyn Future<Output = SignerResult<BuildAndPostOfferResponse>> + Send>>;

/// Boxed future for native manager CLI commands (including coin-op subcommands).
pub type ManagerCommandFuture<'a> = Pin<Box<dyn Future<Output = SignerResult<i32>> + 'a>>;

/// Boxed future for vault CAT offer construction (`greenfloor-engine create-offer`).
pub type BuildVaultCatOfferFuture =
    Pin<Box<dyn Future<Output = SignerResult<CreateOfferResult>> + Send>>;

/// Boxed future for daemon strategy offer dispatch.
pub type StrategyDispatchFuture<'a> =
    Pin<Box<dyn Future<Output = SignerResult<OfferDispatchOutput>> + 'a>>;

/// Boxed future for a single managed planned offer post.
pub type ManagedOfferPostFuture<'a> = Pin<Box<dyn Future<Output = SignerResult<bool>> + Send + 'a>>;

/// Boxed future for daemon managed coin-op plan execution.
pub type ManagedCoinOpPlansFuture<'a> =
    Pin<Box<dyn Future<Output = CoinOpExecutionResult> + Send + 'a>>;
