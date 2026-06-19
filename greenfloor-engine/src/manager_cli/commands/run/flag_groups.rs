use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::flag_groups;

use super::super::clap::ManagerCommands;

pub fn run_command(command: ManagerCommands, ctx: &ManagerContext) -> SignerResult<i32> {
    let ManagerCommands::FlagGroups { subcommand } = command else {
        unreachable!("flag_groups::run_command called with {command:?}");
    };
    let payload = flag_groups::emit_flag_groups(&subcommand)?;
    ctx.emit_json(&payload)?;
    Ok(0)
}
