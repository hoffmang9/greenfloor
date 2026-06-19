//! Setup and config dispatch handlers.

use crate::error::SignerResult;

use super::super::super::commands::ManagerCommands;
use super::super::super::context::ManagerContext;
use super::super::super::keys;
use super::super::super::setup;

pub fn dispatch_setup_command(ctx: &ManagerContext, command: ManagerCommands) -> SignerResult<i32> {
    match command {
        ManagerCommands::ConfigValidate { program_only } => {
            setup::run_config_validate(ctx, program_only)
        }
        ManagerCommands::ProgramFields => setup::run_program_fields(ctx),
        ManagerCommands::MarketsFields => setup::run_markets_fields(ctx),
        ManagerCommands::CatsFields => setup::run_cats_fields(ctx),
        ManagerCommands::MaterializeMinimalProgram {
            output,
            home_dir,
            dexie_api_base,
            log_level,
            dry_run,
            low_inventory_alerts_enabled,
            pushover_enabled,
            with_signer,
        } => Ok(setup::run_materialize_minimal_program(
            setup::MaterializeMinimalProgramRequest {
                output: &output,
                home_dir: &home_dir,
                dexie_api_base: &dexie_api_base,
                log_level: &log_level,
                features: setup::MaterializeMinimalProgramFeatureFlags {
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
            super::super::super::paths::optional_path(&chia_keys_dir).as_deref(),
        ),
        ManagerCommands::Doctor => setup::run_doctor(ctx),
        ManagerCommands::BootstrapHome {
            home_dir,
            program_template,
            markets_template,
            cats_template,
            testnet_markets_template,
            seed_testnet_markets,
            force,
        } => setup::run_bootstrap_home(&setup::BootstrapHomeParams {
            ctx,
            home_dir: &home_dir,
            program_template: &program_template,
            markets_template: &markets_template,
            cats_template: super::super::super::paths::optional_path(&cats_template).as_deref(),
            testnet_markets_template: super::super::super::paths::optional_path(
                &testnet_markets_template,
            )
            .as_deref(),
            seed_testnet_markets,
            force,
        }),
        ManagerCommands::SetLogLevel { log_level } => setup::run_set_log_level(ctx, &log_level),
        other => Err(crate::error::SignerError::Other(format!(
            "unexpected setup command: {other:?}"
        ))),
    }
}
