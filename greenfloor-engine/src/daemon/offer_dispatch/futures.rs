//! Typed async boundaries for daemon offer dispatch.

use std::future::Future;
use std::pin::Pin;

use crate::error::SignerResult;

use super::OfferDispatchOutput;

/// Boxed future for daemon strategy offer dispatch.
pub type StrategyDispatchFuture<'a> =
    Pin<Box<dyn Future<Output = SignerResult<OfferDispatchOutput>> + 'a>>;

/// Boxed future for a single managed planned offer post.
pub type ManagedOfferPostFuture<'a> = Pin<Box<dyn Future<Output = SignerResult<bool>> + Send + 'a>>;
