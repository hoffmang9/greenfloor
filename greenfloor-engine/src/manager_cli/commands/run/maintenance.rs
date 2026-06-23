use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::maintenance::run_audit_prune;

use super::super::clap::ManagerCommands;

pub fn run_command(command: ManagerCommands, ctx: &ManagerContext) -> SignerResult<i32> {
    match command {
        ManagerCommands::AuditPrune { dry_run, vacuum } => run_audit_prune(ctx, dry_run, vacuum),
        other => unreachable!("maintenance::run_command called with {other:?}"),
    }
}
