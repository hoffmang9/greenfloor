use std::future::Future;
use std::pin::Pin;

use crate::error::SignerResult;

/// Boxed future for native manager CLI commands (including coin-op subcommands).
pub type ManagerCommandFuture<'a> = Pin<Box<dyn Future<Output = SignerResult<i32>> + 'a>>;
