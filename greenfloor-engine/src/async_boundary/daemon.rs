use std::future::Future;
use std::pin::Pin;

use crate::daemon::{CoinOpExecutionResult, OfferDispatchOutput};
use crate::error::SignerResult;

/// Boxed future for daemon strategy offer dispatch.
pub type StrategyDispatchFuture<'a> =
    Pin<Box<dyn Future<Output = SignerResult<OfferDispatchOutput>> + 'a>>;

/// Boxed future for a single managed planned offer post.
pub type ManagedOfferPostFuture<'a> = Pin<Box<dyn Future<Output = SignerResult<bool>> + Send + 'a>>;

/// Boxed future for daemon managed coin-op plan execution.
pub type ManagedCoinOpPlansFuture<'a> =
    Pin<Box<dyn Future<Output = CoinOpExecutionResult> + Send + 'a>>;
