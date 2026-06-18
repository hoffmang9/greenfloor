//! Command dispatch for the native manager CLI.

use std::path::{Path, PathBuf};

use crate::error::SignerResult;
use crate::offer::operator::{build_and_post_offer, BuildAndPostOfferRequest, OfferOperatorTestOverrides};

use super::commands::{ManagerCli, ManagerCommands};
use super::offers::{
    run_offers_cancel_command, run_offers_reconcile_command, run_offers_status_command,
    OffersCancelCliArgs, OffersReconcileCliArgs, OffersStatusCliArgs,
};
use super::paths::{
    default_cats_config_path, default_markets_config_path, default_program_config_path,
    default_testnet_markets_config_path, optional_path, resolve_cli_config_path,
};
use super::util::require_market_selector;
use crate::cli_util::optional_str;
use super::{cats, coin_op_loop, json, keys, setup};

pub async fn run_manager_cli(cli: ManagerCli) -> SignerResult<i32> {
    let cli = resolve_manager_cli_paths(cli);
    json::set_json_output_compact(cli.json);
    let testnet_markets_path = resolve_testnet_markets_path(&cli);
    match cli.command {
        ManagerCommands::ConfigValidate => setup::run_config_validate(
            &cli.program_config,
            &cli.markets_config,
            testnet_markets_path.as_deref(),
        ),
        ManagerCommands::KeysOnboard {
            chia_keys_dir,
            key_id,
            state_dir,
        } => keys::run_keys_onboard(
            &cli.program_config,
            &key_id,
            &state_dir,
            optional_path(&chia_keys_dir).as_deref(),
        ),
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
            require_market_selector(market_id.as_deref(), pair.as_deref())?;
            let response = build_and_post_offer(BuildAndPostOfferRequest {
                program_path: cli.program_config,
                markets_path: cli.markets_config,
                testnet_markets_path: testnet_markets_path,
                network,
                market_id,
                pair,
                size_base_units,
                repeat,
                publish_venue: venue,
                dexie_base_url: dexie_base_url.or(cli.dexie_base_url),
                splash_base_url,
                drop_only: !allow_take,
                claim_rewards,
                dry_run,
                compact_json: cli.json,
                persist_results: true,
                action_side: None,
                test_overrides: OfferOperatorTestOverrides::from_env(),
            })
            .await?;
            json::emit_json(&response.payload)?;
            Ok(response.exit_code)
        }
        ManagerCommands::Doctor => setup::run_doctor(
            &cli.program_config,
            &cli.markets_config,
            optional_path(&cli.state_db)
                .as_deref()
                .and_then(|p| p.to_str()),
            testnet_markets_path.as_deref(),
        ),
        ManagerCommands::OffersStatus {
            market_id,
            limit,
            events_limit,
        } => run_offers_status_command(OffersStatusCliArgs {
            program_config: cli.program_config,
            state_db: cli.state_db,
            market_id,
            limit,
            events_limit,
        }),
        ManagerCommands::OffersReconcile {
            market_id,
            limit,
            venue,
        } => {
            run_offers_reconcile_command(OffersReconcileCliArgs {
                program_config: cli.program_config,
                state_db: cli.state_db,
                market_id,
                limit,
                venue,
            })
            .await
        }
        ManagerCommands::OffersCancel {
            offer_id,
            cancel_open,
            venue,
        } => {
            run_offers_cancel_command(OffersCancelCliArgs {
                program_config: cli.program_config,
                offer_id,
                cancel_open,
                venue,
            })
            .await
        }
        ManagerCommands::BootstrapHome {
            home_dir,
            program_template,
            markets_template,
            cats_template,
            testnet_markets_template,
            seed_testnet_markets,
            force,
        } => setup::run_bootstrap_home(
            &home_dir,
            &program_template,
            &markets_template,
            optional_path(&cats_template).as_deref(),
            optional_path(&testnet_markets_template).as_deref(),
            seed_testnet_markets,
            force,
        ),
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
                &cli.cats_config,
                &network,
                cat_id.as_deref(),
                ticker.as_deref(),
                name.as_deref(),
                base_symbol.as_deref(),
                ticker_id.as_deref(),
                pool_id.as_deref(),
                last_price_xch.as_deref(),
                target_usd_per_unit.as_deref(),
                !no_dexie_lookup,
                replace,
                cli.dexie_base_url.as_deref(),
            )
            .await
        }
        ManagerCommands::CatsList => cats::run_cats_list(&cli.cats_config).await,
        ManagerCommands::CatsDelete {
            network,
            cat_id,
            ticker,
            no_dexie_lookup,
            yes,
            preflight_only,
        } => {
            cats::run_cats_delete(
                &cli.cats_config,
                &network,
                cat_id.as_deref(),
                ticker.as_deref(),
                !no_dexie_lookup,
                yes,
                preflight_only,
                cli.dexie_base_url.as_deref(),
            )
            .await
        }
        ManagerCommands::SetLogLevel { log_level } => {
            setup::run_set_log_level(&cli.program_config, &log_level)
        }
        ManagerCommands::CoinsList {
            asset,
            vault_id,
            cat_id,
        } => {
            coin_op_loop::run_coins_list(
                &cli.program_config,
                &cli.markets_config,
                optional_str(&asset),
                optional_str(&vault_id),
                optional_str(&cat_id),
            )
            .await
        }
        ManagerCommands::CoinStatus {
            asset,
            vault_id,
            cat_id,
        } => {
            coin_op_loop::run_coin_status(
                &cli.program_config,
                &cli.markets_config,
                optional_str(&asset),
                optional_str(&vault_id),
                optional_str(&cat_id),
            )
            .await
        }
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
            require_market_selector(market_id.as_deref(), pair.as_deref())?;
            coin_op_loop::run_coin_split(
                &cli.program_config,
                &cli.markets_config,
                testnet_markets_path.as_deref(),
                &network,
                market_id.as_deref(),
                pair.as_deref(),
                &coin_id,
                amount_per_coin,
                number_of_coins,
                no_wait,
                if size_base_units > 0 {
                    Some(size_base_units)
                } else {
                    None
                },
                until_ready,
                max_iterations,
                allow_lock_all_spendable,
                force_split_when_ready,
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
            require_market_selector(market_id.as_deref(), pair.as_deref())?;
            coin_op_loop::run_coin_combine(
                &cli.program_config,
                &cli.markets_config,
                testnet_markets_path.as_deref(),
                &network,
                market_id.as_deref(),
                pair.as_deref(),
                &coin_id,
                input_coin_count,
                optional_str(&asset_id),
                no_wait,
                if size_base_units > 0 {
                    Some(size_base_units)
                } else {
                    None
                },
                until_ready,
                max_iterations,
            )
            .await
        }
    }
}

fn resolve_manager_cli_paths(mut cli: ManagerCli) -> ManagerCli {
    cli.program_config = resolve_cli_config_path(
        &cli.program_config,
        Path::new("config/program.yaml"),
        default_program_config_path,
    );
    cli.markets_config = resolve_cli_config_path(
        &cli.markets_config,
        Path::new("config/markets.yaml"),
        default_markets_config_path,
    );
    cli.cats_config = resolve_cli_config_path(
        &cli.cats_config,
        Path::new("config/cats.yaml"),
        default_cats_config_path,
    );
    cli
}

fn resolve_testnet_markets_path(cli: &ManagerCli) -> Option<PathBuf> {
    let explicit = cli.testnet_markets_config.trim();
    if !explicit.is_empty() {
        return Some(PathBuf::from(explicit));
    }
    default_testnet_markets_config_path()
}
