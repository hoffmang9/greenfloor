use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::keys;
use crate::manager_cli::paths;
use crate::manager_cli::setup::{
    run_bootstrap_home, run_cats_fields, run_config_validate, run_doctor, run_markets_fields,
    run_materialize_minimal_program, run_program_fields, run_set_log_level, BootstrapHomeParams,
    MaterializeMinimalProgramFeatureFlags, MaterializeMinimalProgramRequest,
};

use super::super::clap::ManagerCommands;

pub fn run_command(command: ManagerCommands, ctx: &ManagerContext) -> SignerResult<i32> {
    match command {
        ManagerCommands::ConfigValidate { program_only } => run_config_validate(ctx, program_only),
        ManagerCommands::ProgramFields => run_program_fields(ctx),
        ManagerCommands::MarketsFields => run_markets_fields(ctx),
        ManagerCommands::CatsFields => run_cats_fields(ctx),
        ManagerCommands::MaterializeMinimalProgram {
            output,
            home_dir,
            dexie_api_base,
            log_level,
            dry_run,
            low_inventory_alerts_enabled,
            pushover_enabled,
            with_signer,
        } => Ok(run_materialize_minimal_program(
            MaterializeMinimalProgramRequest {
                output: &output,
                home_dir: &home_dir,
                dexie_api_base: &dexie_api_base,
                log_level: &log_level,
                features: MaterializeMinimalProgramFeatureFlags {
                    dry_run,
                    low_inventory_alerts_enabled,
                    pushover_enabled,
                },
                with_signer,
            },
        )),
        ManagerCommands::KeysOnboard {
            chia_keys_dir,
            key_id,
            state_dir,
        } => keys::run_keys_onboard(
            ctx,
            &key_id,
            &state_dir,
            paths::optional_path(&chia_keys_dir).as_deref(),
        ),
        ManagerCommands::Doctor => run_doctor(ctx),
        ManagerCommands::BootstrapHome {
            home_dir,
            program_template,
            markets_template,
            cats_template,
            testnet_markets_template,
            seed_testnet_markets,
            force,
        } => run_bootstrap_home(&BootstrapHomeParams {
            ctx,
            home_dir: &home_dir,
            program_template: &program_template,
            markets_template: &markets_template,
            cats_template: paths::optional_path(&cats_template).as_deref(),
            testnet_markets_template: paths::optional_path(&testnet_markets_template).as_deref(),
            seed_testnet_markets,
            force,
        }),
        ManagerCommands::SetLogLevel { log_level } => run_set_log_level(ctx, &log_level),
        other => unreachable!("setup::run_command called with {other:?}"),
    }
}
