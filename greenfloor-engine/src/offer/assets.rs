//! Offer asset label resolution: hex ids, local ticker index, optional Coinset fallback.

use chia_sdk_coinset::CoinsetClient;

use crate::coinset::{is_xch_like_asset, lookup_asset_by_symbol};
use crate::config::{lookup_asset_id_from_ticker, resolve_quote_asset_for_offer, CatTickerIndex};
use crate::config::{MarketConfig, SignerConfig};
use crate::error::{SignerError, SignerResult};

/// Resolved on-chain asset ids for a configured market row (offer build / reservations).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMarketOfferAssets {
    pub base_asset_id: String,
    pub quote_asset_id: String,
    /// Network-normalized quote leg label used for asset resolution and fee routing.
    pub quote_asset_for_offer: String,
}

/// Signer-backed offer asset resolution using a pre-built operator ticker index.
pub struct OfferAssetResolver<'a> {
    pub(crate) signer: &'a SignerConfig,
    pub(crate) index: &'a CatTickerIndex,
    pub(crate) operator_network: &'a str,
}

impl<'a> OfferAssetResolver<'a> {
    #[must_use]
    pub fn new(
        signer: &'a SignerConfig,
        index: &'a CatTickerIndex,
        operator_network: &'a str,
    ) -> Self {
        Self {
            signer,
            index,
            operator_network,
        }
    }

    fn coinset_client(&self) -> SignerResult<CoinsetClient> {
        crate::coinset::client_for_signer_on_network(self.signer, self.operator_network)
    }

    /// Resolve offer base/quote labels to on-chain asset ids.
    ///
    /// # Errors
    ///
    /// Returns an error if asset resolution fails.
    pub async fn resolve_pair(
        &self,
        base_asset: &str,
        quote_asset: &str,
    ) -> SignerResult<(String, String)> {
        let client = self.coinset_client()?;
        resolve_offer_asset_ids(&client, base_asset, quote_asset, self.index).await
    }

    /// Resolve a base asset label (coin-op / inventory paths use xch as quote leg).
    ///
    /// # Errors
    ///
    /// Returns an error if asset resolution fails.
    pub async fn resolve_base(&self, base_asset: &str) -> SignerResult<String> {
        self.resolve_pair(base_asset.trim(), "xch")
            .await
            .map(|(base, _)| base)
    }

    /// Resolve base and quote asset ids for a configured market row.
    ///
    /// # Errors
    ///
    /// Returns an error if asset resolution fails.
    pub async fn resolve_market_assets(
        &self,
        market: &MarketConfig,
    ) -> SignerResult<ResolvedMarketOfferAssets> {
        let quote_asset_for_offer =
            resolve_quote_asset_for_offer(market.quote_asset.trim(), self.operator_network);
        let (base_asset_id, quote_asset_id) = self
            .resolve_pair(market.base_asset.trim(), &quote_asset_for_offer)
            .await?;
        Ok(ResolvedMarketOfferAssets {
            base_asset_id,
            quote_asset_id,
            quote_asset_for_offer,
        })
    }

    /// Resolve fee-leg asset id for parallel offer reservations.
    ///
    /// # Errors
    ///
    /// Returns an error if asset resolution fails.
    pub async fn resolve_fee_asset(
        &self,
        assets: &ResolvedMarketOfferAssets,
    ) -> SignerResult<String> {
        if is_xch_like_asset(&assets.quote_asset_for_offer) {
            return Ok(assets.quote_asset_id.clone());
        }
        Ok(self
            .resolve_pair("xch", &assets.quote_asset_for_offer)
            .await?
            .0)
    }
}

/// Resolve offer base/quote labels to on-chain asset ids.
///
/// # Errors
///
/// Returns an error if asset resolution fails.
pub async fn resolve_offer_assets(
    config: &SignerConfig,
    base_asset: &str,
    quote_asset: &str,
    ticker_index: &CatTickerIndex,
    operator_network: &str,
) -> SignerResult<(String, String)> {
    OfferAssetResolver::new(config, ticker_index, operator_network)
        .resolve_pair(base_asset, quote_asset)
        .await
}

/// Resolve offer asset ids using an explicit Coinset client and ticker index.
///
/// # Errors
///
/// Returns an error if asset resolution fails.
pub async fn resolve_offer_asset_ids(
    client: &CoinsetClient,
    base_asset: &str,
    quote_asset: &str,
    ticker_index: &CatTickerIndex,
) -> SignerResult<(String, String)> {
    let resolved_base = resolve_one_asset(client, base_asset, ticker_index).await?;
    let resolved_quote = resolve_one_asset(client, quote_asset, ticker_index).await?;
    ensure_distinct_non_xch_pair(&resolved_base, &resolved_quote)?;
    Ok((resolved_base, resolved_quote))
}

fn ensure_distinct_non_xch_pair(base: &str, quote: &str) -> SignerResult<()> {
    if base == quote && !is_xch_like_asset(base) && !is_xch_like_asset(quote) {
        return Err(SignerError::ResolvedAssetsCollideForNonXchPair);
    }
    Ok(())
}

/// Normalize a resolved offer asset id (XCH/TXCH label or 64-hex CAT id).
///
/// # Errors
///
/// Returns an error if the asset id is invalid.
pub fn normalize_asset_id(raw: &str) -> SignerResult<String> {
    let trimmed = raw.trim().to_lowercase();
    if trimmed.is_empty() {
        return Err(SignerError::MissingAssetId);
    }
    if matches!(trimmed.as_str(), "xch" | "txch" | "1") {
        return Ok(trimmed);
    }
    if trimmed.len() == 64 && trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Ok(trimmed);
    }
    Err(SignerError::Other(format!(
        "invalid asset id (expected 64-hex cat id or xch/txch): {raw}"
    )))
}

async fn resolve_one_asset(
    client: &CoinsetClient,
    raw: &str,
    ticker_index: &CatTickerIndex,
) -> SignerResult<String> {
    if let Ok(normalized) = normalize_asset_id(raw) {
        return Ok(normalized);
    }
    if let Some(asset_id) = lookup_asset_id_from_ticker(ticker_index, raw)? {
        return normalize_asset_id(&asset_id);
    }
    match lookup_asset_by_symbol(client, raw).await {
        Ok(Some(asset)) => normalize_asset_id(&asset.asset_id),
        Ok(None) => Err(SignerError::Other(format!("asset_resolution_failed:{raw}"))),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::build_cat_ticker_index_lenient;
    use chia_sdk_coinset::CoinsetClient;

    #[test]
    fn normalize_asset_id_accepts_xch_and_hex() {
        assert_eq!(normalize_asset_id("xch").unwrap(), "xch");
        assert_eq!(normalize_asset_id("TXCH").unwrap(), "txch");
        let cat = "a".repeat(64);
        assert_eq!(normalize_asset_id(&cat).unwrap(), cat);
    }

    #[test]
    fn normalize_asset_id_rejects_invalid() {
        assert!(normalize_asset_id("").is_err());
        assert!(normalize_asset_id("Asset_foo").is_err());
    }

    #[test]
    fn ensure_distinct_non_xch_pair_rejects_identical_cats() {
        let cat = "a".repeat(64);
        let err = ensure_distinct_non_xch_pair(&cat, &cat).expect_err("collision");
        assert!(matches!(
            err,
            SignerError::ResolvedAssetsCollideForNonXchPair
        ));
    }

    #[test]
    fn ensure_distinct_non_xch_pair_allows_xch_pair() {
        let cat = "a".repeat(64);
        ensure_distinct_non_xch_pair(&cat, "xch").expect("xch leg");
    }

    #[tokio::test]
    async fn resolve_offer_asset_ids_uses_ticker_index_before_coinset() {
        let asset_id = "cc".repeat(32);
        let dir = tempfile::tempdir().expect("tempdir");
        let cats = dir.path().join("cats.yaml");
        std::fs::write(
            &cats,
            format!(
                r"
cats:
  - asset_id: {asset_id}
    base_symbol: HOA
"
            ),
        )
        .expect("write cats");
        let markets = dir.path().join("markets.yaml");
        std::fs::write(&markets, "markets: []\n").expect("write markets");
        let ticker_index = build_cat_ticker_index_lenient(&cats, &markets, None);
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/lookup_asset_by_symbol")
            .expect(0)
            .create_async()
            .await;
        let client = CoinsetClient::new(server.url());
        let (base, quote) = resolve_offer_asset_ids(&client, "HOA", "xch", &ticker_index)
            .await
            .expect("resolved");
        assert_eq!(base, asset_id);
        assert_eq!(quote, "xch");
    }

    #[tokio::test]
    async fn resolver_resolves_byc_wusdc_from_index_without_coinset() {
        let byc_id = "ae1536f56760e471ad85ead45f00d680ff9cca73b8cc3407be778f1c0c606eac";
        let wusdc_id = "fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d";
        let dir = tempfile::tempdir().expect("tempdir");
        let cats = dir.path().join("cats.yaml");
        std::fs::write(
            &cats,
            format!(
                r"
cats:
  - asset_id: {byc_id}
    base_symbol: BYC
  - asset_id: {wusdc_id}
    base_symbol: wUSDC.b
"
            ),
        )
        .expect("write cats");
        let markets = dir.path().join("markets.yaml");
        std::fs::write(&markets, "markets: []\n").expect("write markets");
        let index = build_cat_ticker_index_lenient(&cats, &markets, None);
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/lookup_asset_by_symbol")
            .expect(0)
            .create_async()
            .await;
        let config = crate::test_support::signer_config::test_signer_config(&server.url());
        let (base, quote) = OfferAssetResolver::new(&config, &index, "mainnet")
            .resolve_pair("BYC", "wUSDC.b")
            .await
            .expect("ticker index resolution");
        assert_eq!(base, byc_id);
        assert_eq!(quote, wusdc_id);
    }

    #[tokio::test]
    async fn resolve_one_asset_propagates_coinset_transport_errors() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/lookup_asset_by_symbol")
            .with_status(500)
            .create_async()
            .await;
        let client = CoinsetClient::new(server.url());
        let err = resolve_offer_asset_ids(
            &client,
            "HOA",
            "xch",
            &crate::config::empty_cat_ticker_index(),
        )
        .await
        .expect_err("transport error");
        assert!(!err.to_string().contains("asset_resolution_failed"));
    }

    #[tokio::test]
    async fn resolver_falls_back_to_coinset_for_unknown_ticker() {
        let mut server = mockito::Server::new_async().await;
        let cat_id = "b".repeat(64);
        let _mock = server
            .mock("POST", "/lookup_asset_by_symbol")
            .with_status(200)
            .with_body(format!(
                r#"{{"success":true,"asset":{{"asset_id":"{cat_id}","symbol":"HOA"}}}}"#
            ))
            .create_async()
            .await;
        let config = crate::test_support::signer_config::test_signer_config(&server.url());
        let (base, quote) =
            OfferAssetResolver::new(&config, &crate::config::empty_cat_ticker_index(), "mainnet")
                .resolve_pair("HOA", "xch")
                .await
                .expect("coinset resolution");
        assert_eq!(base, cat_id);
        assert_eq!(quote, "xch");
    }
}
