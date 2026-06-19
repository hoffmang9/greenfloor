//! Typed async boundaries for hot operator paths.

use std::future::Future;
use std::pin::Pin;

use crate::error::SignerResult;
use crate::offer::operator::BuildAndPostOfferResponse;

/// Boxed future for managed offer build/post (daemon + CLI).
pub type BuildAndPostOfferFuture =
    Pin<Box<dyn Future<Output = SignerResult<BuildAndPostOfferResponse>> + Send>>;

/// Boxed future for a native manager CLI command.
pub type ManagerCommandFuture<'a> = Pin<Box<dyn Future<Output = SignerResult<i32>> + 'a>>;

/// Boxed future for coin-op CLI commands.
pub type CoinOpCommandFuture<'a> = Pin<Box<dyn Future<Output = SignerResult<i32>> + 'a>>;
