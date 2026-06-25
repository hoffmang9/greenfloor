//! Runtime config loaders for operator and daemon entrypoints.
//!
//! Use the loader that matches the caller's signer gate and market resolution needs:
//!
//! | Loader | Entrypoints | Signer parse | Resolves one market |
//! | --- | --- | --- | --- |
//! | [`load_gated_operator_market`] | `build-and-post-offer`, coin-op CLI, `coins-list` | hard fail on missing signer | yes (see [`OperatorMarketCommand`]) |
//! | [`load_daemon_cycle_config`] | daemon cycle (`load_cycle_resources`) | soft (`CycleProgramConfig`) | no (full markets list) |
//! | [`load_combine_command_resources`] | `combine-market-cat-dust` | gated signer when executing | no (full markets list) |
//!
//! Asset id resolution after load uses [`crate::offer::OfferAssetResolver`] built from
//! [`GatedOperatorMarket::asset_resolver`] or [`operator_ticker_index_from_paths`].

use std::path::Path;

use serde_json::Value;

use super::cat_ticker_index::{build_cat_ticker_index_lenient, CatTickerIndex};
use super::{
    ensure_market_receive_address_for_network, load_markets_config_with_overlay,
    load_program_bundle_gated, parse_program_config, parse_signer_config, read_program_yaml,
    resolve_market_for_build, select_coin_list_market, CycleProgramConfig, ManagerProgramConfig,
    MarketConfig, MarketsConfig, SignerConfig,
};
use crate::coinset::{resolve_coinset_endpoint, ResolvedCoinsetEndpoint, DEFAULT_COINSET_BASE_URL};
use crate::error::SignerResult;
use crate::paths::resolve_cats_config_path;

/// Which market-resolution rules apply when loading a single operator market row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorMarketCommand {
    /// `build-and-post-offer`, coin-split/combine: require exactly one of `--market-id` or `--pair`.
    Build,
    /// `coins-list` / `coin-status`: allow default market for the operator network.
    CoinList,
}

/// Gated program bundle plus one resolved market row (manager CLI operator commands).
#[derive(Debug, Clone)]
pub struct GatedOperatorMarket {
    pub program: ManagerProgramConfig,
    pub signer: SignerConfig,
    pub market_row: MarketConfig,
    pub ticker_index: CatTickerIndex,
    pub operator_network: String,
}

impl GatedOperatorMarket {
    #[must_use]
    pub fn assemble(
        program: ManagerProgramConfig,
        signer: SignerConfig,
        market_row: MarketConfig,
        ticker_index: CatTickerIndex,
        operator_network: impl AsRef<str>,
    ) -> Self {
        Self {
            program,
            signer,
            market_row,
            ticker_index,
            operator_network: operator_network.as_ref().trim().to_string(),
        }
    }

    #[must_use]
    pub fn asset_resolver(&self) -> crate::offer::OfferAssetResolver<'_> {
        crate::offer::OfferAssetResolver::new(
            &self.signer,
            &self.ticker_index,
            &self.operator_network,
        )
    }
}

/// Build the operator ticker index from resolved metadata config paths.
#[must_use]
pub fn operator_ticker_index_from_paths(
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    cats_path: Option<&Path>,
) -> CatTickerIndex {
    let cats = resolve_cats_config_path(markets_path, cats_path);
    build_cat_ticker_index_lenient(&cats, markets_path, testnet_markets_path)
}

/// Program and markets config loaded once per daemon cycle.
#[derive(Debug, Clone)]
pub struct DaemonCycleConfig {
    pub program_config: CycleProgramConfig,
    pub markets: MarketsConfig,
    pub network: String,
}

/// Program, markets, Coinset endpoint, and optional execution signer for combine-market-cat-dust.
#[derive(Debug, Clone)]
pub struct CombineCommandResources {
    pub program: ManagerProgramConfig,
    pub markets: MarketsConfig,
    pub coinset: ResolvedCoinsetEndpoint,
    pub execution_signer: Option<SignerConfig>,
}

/// Inputs for [`load_combine_command_resources`].
#[derive(Debug, Clone, Copy)]
pub struct CombineCommandLoadRequest<'a> {
    pub program_path: &'a Path,
    pub markets_path: &'a Path,
    pub testnet_markets_path: Option<&'a Path>,
    pub request_network: Option<&'a str>,
    pub coinset_base_url: Option<&'a str>,
    pub preview_mode: bool,
}

/// Inputs for [`load_gated_operator_market`].
#[derive(Debug, Clone, Copy)]
pub struct GatedOperatorMarketLoadRequest<'a> {
    pub program_path: &'a Path,
    pub markets_path: &'a Path,
    pub testnet_markets_path: Option<&'a Path>,
    pub cats_path: Option<&'a Path>,
    pub network: &'a str,
    pub market_id: Option<&'a str>,
    pub pair: Option<&'a str>,
    pub command: OperatorMarketCommand,
}

/// Load gated program config and resolve one market row for build/coin-op commands.
///
/// # Errors
///
/// Returns an error if config loading or market resolution fails.
pub fn load_gated_operator_market(
    request: &GatedOperatorMarketLoadRequest<'_>,
) -> SignerResult<GatedOperatorMarket> {
    let GatedOperatorMarketLoadRequest {
        program_path,
        markets_path,
        testnet_markets_path,
        cats_path,
        network,
        market_id,
        pair,
        command,
    } = request;
    let bundle = load_program_bundle_gated(program_path)?;
    let markets = load_markets_config_with_overlay(markets_path, *testnet_markets_path)?;
    let market = resolve_operator_market(&markets, network, *market_id, *pair, *command)?;
    let ticker_index =
        operator_ticker_index_from_paths(markets_path, *testnet_markets_path, *cats_path);
    Ok(GatedOperatorMarket::assemble(
        bundle.program,
        bundle.signer,
        market,
        ticker_index,
        network,
    ))
}

fn resolve_operator_market(
    markets: &MarketsConfig,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    command: OperatorMarketCommand,
) -> SignerResult<MarketConfig> {
    let market = match command {
        OperatorMarketCommand::Build => {
            resolve_market_for_build(markets, market_id, pair, network)?
        }
        OperatorMarketCommand::CoinList => {
            select_coin_list_market(markets, network, market_id, pair)?
        }
    };
    ensure_market_receive_address_for_network(&market, network)?;
    Ok(market)
}

/// Load daemon cycle program and markets config (soft signer parse).
///
/// # Errors
///
/// Returns an error if config loading fails.
pub fn load_daemon_cycle_config(
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
) -> SignerResult<DaemonCycleConfig> {
    let raw = read_program_yaml(program_path)?;
    let program = parse_program_config(&raw)?;
    let program_config = CycleProgramConfig::from_parsed(program, &raw);
    let network = program_config.program().network.clone();
    let markets = load_markets_config_with_overlay(markets_path, testnet_markets_path)?;
    Ok(DaemonCycleConfig {
        program_config,
        markets,
        network,
    })
}

fn configured_coinset_base_url(raw: &Value, signer: Option<&SignerConfig>) -> String {
    if let Some(signer) = signer {
        return signer.coinset_base_url.clone();
    }
    parse_signer_config(raw).map_or_else(
        |_| DEFAULT_COINSET_BASE_URL.to_string(),
        |cfg| cfg.coinset_base_url,
    )
}

/// Load program, markets, Coinset endpoint, and optional execution signer for combine.
///
/// # Errors
///
/// Returns an error if config loading fails.
pub fn load_combine_command_resources(
    request: &CombineCommandLoadRequest<'_>,
) -> SignerResult<CombineCommandResources> {
    let CombineCommandLoadRequest {
        program_path,
        markets_path,
        testnet_markets_path,
        request_network,
        coinset_base_url,
        preview_mode,
    } = request;
    let markets = load_markets_config_with_overlay(markets_path, *testnet_markets_path)?;
    let (program, execution_signer, configured_url) = if *preview_mode {
        let raw_program = read_program_yaml(program_path)?;
        let program = parse_program_config(&raw_program)?;
        let configured_url = configured_coinset_base_url(&raw_program, None);
        (program, None, configured_url)
    } else {
        let bundle = load_program_bundle_gated(program_path)?;
        let configured_url = bundle.signer.coinset_base_url.clone();
        (bundle.program, Some(bundle.signer), configured_url)
    };
    let network_source = request_network
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(program.network.as_str());
    let coinset = resolve_coinset_endpoint(network_source, &configured_url, *coinset_base_url);
    Ok(CombineCommandResources {
        program,
        markets,
        coinset,
        execution_signer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::minimal_program::{
        write_minimal_program_with_signer, MinimalProgramParams,
    };
    use tempfile::tempdir;

    fn write_sample_markets(path: &Path) {
        std::fs::write(
            path,
            r"
markets:
  - id: m1
    enabled: true
    base_asset: xch
    base_symbol: XCH
    quote_asset: usdc
    quote_asset_type: stable
    receive_address: xch1test
    signer_key_id: key-main-1
    mode: sell_only
    pricing:
      quote_price: 1.0
    ladders: {}
",
        )
        .expect("write markets");
    }

    #[test]
    fn load_daemon_cycle_config_reads_program_and_markets() {
        let dir = tempdir().expect("tempdir");
        let program_path = dir.path().join("program.yaml");
        write_minimal_program_with_signer(
            &program_path,
            MinimalProgramParams {
                home_dir: dir.path(),
                ..Default::default()
            },
        );
        let markets_path = dir.path().join("markets.yaml");
        write_sample_markets(&markets_path);

        let loaded = load_daemon_cycle_config(&program_path, &markets_path, None).expect("loaded");
        assert_eq!(loaded.network, "mainnet");
        assert_eq!(loaded.markets.markets.len(), 1);
        assert_eq!(loaded.markets.markets[0].market_id, "m1");
    }

    #[test]
    fn load_gated_operator_market_build_rejects_receive_address_network_mismatch() {
        let dir = tempdir().expect("tempdir");
        let program_path = dir.path().join("program.yaml");
        write_minimal_program_with_signer(
            &program_path,
            MinimalProgramParams {
                home_dir: dir.path(),
                ..Default::default()
            },
        );
        let markets_path = dir.path().join("markets.yaml");
        write_sample_markets(&markets_path);

        let err = load_gated_operator_market(&GatedOperatorMarketLoadRequest {
            program_path: &program_path,
            markets_path: &markets_path,
            testnet_markets_path: None,
            cats_path: None,
            network: "testnet11",
            market_id: Some("m1"),
            pair: None,
            command: OperatorMarketCommand::Build,
        })
        .expect_err("mainnet receive_address on testnet11");
        assert!(
            err.to_string()
                .contains("receive_address does not match operator network testnet11"),
            "unexpected error: {err}"
        );
    }
}
