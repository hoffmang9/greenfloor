//! Per-command handlers for the native manager CLI dispatch table.

mod build_and_post;
mod cats;
mod coin_ops;
mod coin_query;

use std::future::Future;
use std::pin::Pin;

use crate::error::SignerResult;
use crate::manager_cli::coin_op_loop::{CoinSplitBehavior, CoinSplitGating, UntilReadyWaitMode};
use crate::manager_cli::commands::ManagerCommands;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::flag_groups;
use crate::manager_cli::keys;
use crate::manager_cli::offers::{
    run_offers_cancel_command, run_offers_reconcile_command, run_offers_status_command,
    OffersCancelCliArgs, OffersReconcileCliArgs, OffersStatusCliArgs,
};
use crate::manager_cli::paths;
use crate::manager_cli::setup::{
    run_bootstrap_home, run_cats_fields, run_config_validate, run_doctor, run_markets_fields,
    run_materialize_minimal_program, run_program_fields, run_set_log_level, BootstrapHomeParams,
    MaterializeMinimalProgramFeatureFlags, MaterializeMinimalProgramRequest,
};

#[allow(clippy::too_many_lines)]
pub fn dispatch_manager_command(
    ctx: &ManagerContext,
    command: ManagerCommands,
) -> Pin<Box<dyn Future<Output = SignerResult<i32>> + '_>> {
    Box::pin(dispatch_manager_command_async(ctx, command))
}

#[allow(clippy::too_many_lines)]
async fn dispatch_manager_command_async(
    ctx: &ManagerContext,
    command: ManagerCommands,
) -> SignerResult<i32> {
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
        ManagerCommands::BuildAndPostOffer {
            market_id,
            pair,
            size_base_units,
            repeat,
            network,
            dexie_base_url,
            allow_take,
            claim_rewards,
            dry_run,
            venue,
            splash_base_url,
        } => {
            build_and_post::run_build_and_post_offer(
                ctx,
                market_id,
                pair,
                size_base_units,
                repeat,
                network,
                dexie_base_url,
                allow_take,
                claim_rewards,
                dry_run,
                venue,
                splash_base_url,
            )
            .await
        }
        ManagerCommands::OffersStatus {
            market_id,
            limit,
            events_limit,
        } => run_offers_status_command(
            ctx,
            &OffersStatusCliArgs {
                market_id,
                limit,
                events_limit,
            },
        ),
        ManagerCommands::OffersReconcile {
            market_id,
            limit,
            venue,
        } => {
            run_offers_reconcile_command(
                ctx,
                OffersReconcileCliArgs {
                    market_id,
                    limit,
                    venue,
                },
            )
            .await
        }
        ManagerCommands::OffersCancel {
            offer_id,
            cancel_open,
            venue,
        } => {
            run_offers_cancel_command(
                ctx,
                OffersCancelCliArgs {
                    offer_id,
                    cancel_open,
                    venue,
                },
            )
            .await
        }
        ManagerCommands::CatsAdd {
            network,
            cat_id,
            ticker,
            name,
            base_symbol,
            ticker_id,
            pool_id,
            last_price_xch,
            target_usd_per_unit,
            no_dexie_lookup,
            replace,
        } => {
            cats::run_cats_add(
                ctx,
                network,
                cat_id,
                ticker,
                name,
                base_symbol,
                ticker_id,
                pool_id,
                last_price_xch,
                target_usd_per_unit,
                no_dexie_lookup,
                replace,
            )
            .await
        }
        ManagerCommands::CatsList => cats::run_cats_list(ctx),
        ManagerCommands::CatsDelete {
            network,
            cat_id,
            ticker,
            no_dexie_lookup,
            yes,
            preflight_only,
        } => {
            cats::run_cats_delete(
                ctx,
                network,
                cat_id,
                ticker,
                no_dexie_lookup,
                yes,
                preflight_only,
            )
            .await
        }
        ManagerCommands::CoinsList {
            asset,
            vault_id,
            cat_id,
        } => coin_query::run_coins_list(ctx, asset, vault_id, cat_id).await,
        ManagerCommands::CoinStatus {
            asset,
            vault_id,
            cat_id,
        } => coin_query::run_coin_status(ctx, asset, vault_id, cat_id).await,
        ManagerCommands::CoinSplit {
            market_id,
            pair,
            network,
            coin_id,
            amount_per_coin,
            number_of_coins,
            size_base_units,
            until_ready,
            max_iterations,
            no_wait,
            allow_lock_all_spendable,
            force_split_when_ready,
        } => {
            coin_ops::run_coin_split(
                ctx,
                market_id,
                pair,
                network,
                coin_id,
                amount_per_coin,
                number_of_coins,
                size_base_units,
                max_iterations,
                CoinSplitBehavior {
                    wait: UntilReadyWaitMode {
                        until_ready,
                        no_wait,
                    },
                    gating: CoinSplitGating {
                        allow_lock_all_spendable,
                        force_split_when_ready,
                    },
                },
            )
            .await
        }
        ManagerCommands::CoinCombine {
            market_id,
            pair,
            network,
            input_coin_count,
            asset_id,
            coin_id,
            size_base_units,
            until_ready,
            max_iterations,
            no_wait,
        } => {
            coin_ops::run_coin_combine(
                ctx,
                market_id,
                pair,
                network,
                input_coin_count,
                asset_id,
                coin_id,
                size_base_units,
                UntilReadyWaitMode {
                    until_ready,
                    no_wait,
                },
                max_iterations,
            )
            .await
        }
        ManagerCommands::CombineMarketCatDust {
            network,
            coinset_base_url,
            launcher_id,
            launcher_id_file,
            dust_threshold_mojos,
            max_input_coins,
            max_nonce,
            cat_asset_id,
            dry_run,
            list_only,
            verify_timeout_seconds,
            verify_poll_seconds,
        } => {
            coin_ops::run_combine_market_cat_dust(
                ctx,
                network,
                coinset_base_url,
                launcher_id,
                launcher_id_file,
                dust_threshold_mojos,
                max_input_coins,
                max_nonce,
                cat_asset_id,
                dry_run,
                list_only,
                verify_timeout_seconds,
                verify_poll_seconds,
            )
            .await
        }
        ManagerCommands::FlagGroups { subcommand } => {
            let payload = flag_groups::emit_flag_groups(&subcommand)?;
            ctx.emit_json(&payload)?;
            Ok(0)
        }
    }
}
