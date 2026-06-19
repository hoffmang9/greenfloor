use std::future::Future;
use std::pin::Pin;

use crate::error::SignerResult;

/// Boxed future for native manager CLI commands (including coin-op subcommands).
///
/// Intentionally not `Send`: manager paths may hold a non-`Sync` `SqliteStore` borrow
/// across the command future. Do not spawn this future onto a multithreaded runtime
/// without cloning state onto the heap first.
pub type ManagerCommandFuture<'a> = Pin<Box<dyn Future<Output = SignerResult<i32>> + 'a>>;
