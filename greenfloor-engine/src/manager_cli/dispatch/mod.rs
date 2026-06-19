//! Command dispatch for the native manager CLI.

mod handlers;

use crate::error::SignerResult;

use super::commands::ManagerCli;
use super::context::ManagerContext;

/// Run manager cli.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_manager_cli(cli: ManagerCli) -> SignerResult<i32> {
    let (ctx, command) = ManagerContext::from_cli(cli);
    handlers::dispatch_manager_command(&ctx, command).await
}
