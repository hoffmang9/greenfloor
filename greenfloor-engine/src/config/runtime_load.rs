//! Runtime config loaders for operator and daemon entrypoints.
//!
//! Use the loader that matches the caller's signer gate and market resolution needs:
//!
//! | Loader | Entrypoints | Signer parse | Resolves one market |
//! | --- | --- | --- | --- |
//! | [`load_gated_operator_market`] | `build-and-post-offer`, coin-op CLI | hard fail on missing signer | yes (`market_id` / `pair` / `network`) |
//! | [`load_daemon_cycle_config`] | daemon cycle (`load_cycle_resources`) | soft (`CycleProgramConfig`) | no (full markets list) |
//! | [`load_raw_program_and_markets`] | `combine-market-cat-dust` (needs raw YAML) | parse program only | no (full markets list) |
//!
//! Asset id resolution after load is separate: coin-op/inventory use
//! [`crate::offer::resolve_market_base_asset_id`] with an explicit [`CatTickerIndex`]; offer build
//! and reservations use [`crate::offer::resolve_market_offer_assets_for_action`]. Ticker symbols
//! resolve via the index built from operator metadata paths before Coinset fallback.

use std::path::Path;

use serde_json::Value;

use super::cat_ticker_index::{build_cat_ticker_index_lenient, CatTickerIndex};
use super::{
    load_markets_config_with_overlay, load_program_bundle_gated, parse_program_config,
    read_program_yaml, resolve_market_for_build, CycleProgramConfig, ManagerProgramConfig,
    MarketConfig, MarketsConfig, SignerConfig,
};
use crate::error::SignerResult;
use crate::paths::resolve_cats_config_path;

/// Gated program bundle plus one resolved market row (manager CLI operator commands).
#[derive(Debug, Clone)]
pub struct GatedOperatorMarket {
    pub program: ManagerProgramConfig,
    pub signer: SignerConfig,
    pub market: MarketConfig,
    pub ticker_index: CatTickerIndex,
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

/// Parsed program plus markets when callers also need raw program YAML.
#[derive(Debug, Clone)]
pub struct RawProgramMarkets {
    pub raw_program: Value,
    pub program: ManagerProgramConfig,
    pub markets: MarketsConfig,
}

/// Load gated program config and resolve one market row for build/coin-op commands.
///
/// # Errors
///
/// Returns an error if config loading or market resolution fails.
pub fn load_gated_operator_market(
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    cats_path: Option<&Path>,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
) -> SignerResult<GatedOperatorMarket> {
    let bundle = load_program_bundle_gated(program_path)?;
    let markets = load_markets_config_with_overlay(markets_path, testnet_markets_path)?;
    let market = resolve_market_for_build(&markets, market_id, pair, network)?;
    let ticker_index =
        operator_ticker_index_from_paths(markets_path, testnet_markets_path, cats_path);
    Ok(GatedOperatorMarket {
        program: bundle.program,
        signer: bundle.signer,
        market,
        ticker_index,
    })
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

/// Load parsed program and markets while retaining raw program YAML.
///
/// # Errors
///
/// Returns an error if config loading fails.
pub fn load_raw_program_and_markets(
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
) -> SignerResult<RawProgramMarkets> {
    let raw = read_program_yaml(program_path)?;
    let program = parse_program_config(&raw)?;
    let markets = load_markets_config_with_overlay(markets_path, testnet_markets_path)?;
    Ok(RawProgramMarkets {
        raw_program: raw,
        program,
        markets,
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
    fn load_raw_program_and_markets_retains_yaml() {
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

        let loaded =
            load_raw_program_and_markets(&program_path, &markets_path, None).expect("loaded");
        assert!(loaded.raw_program.is_object());
        assert_eq!(loaded.program.network, "mainnet");
        assert_eq!(loaded.markets.markets.len(), 1);
    }
}
