//! Offer asset label resolution: hex ids, local ticker index, optional Coinset fallback.

use chia_sdk_coinset::CoinsetClient;

use crate::coinset::{
    client_for_signer, is_xch_like_asset, lookup_asset_by_symbol, normalize_asset_id,
};
use crate::config::{lookup_asset_id_from_ticker, CatTickerIndex, SignerConfig};
use crate::error::{SignerError, SignerResult};

/// Resolve offer base/quote labels to on-chain asset ids.
///
/// Order: hex/xch normalization, local ticker index, then Coinset `lookup_asset_by_symbol`.
///
/// # Errors
///
/// Returns an error if asset resolution fails.
pub async fn resolve_offer_assets(
    config: &SignerConfig,
    base_asset: &str,
    quote_asset: &str,
    ticker_index: &CatTickerIndex,
) -> SignerResult<(String, String)> {
    let client = client_for_signer(config)?;
    resolve_offer_asset_ids(&client, base_asset, quote_asset, ticker_index).await
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
    if resolved_base == resolved_quote
        && !is_xch_like_asset(&resolved_base)
        && !is_xch_like_asset(&resolved_quote)
    {
        return Err(SignerError::ResolvedAssetsCollideForNonXchPair);
    }
    Ok((resolved_base, resolved_quote))
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
    if let Ok(Some(asset)) = lookup_asset_by_symbol(client, raw).await {
        return normalize_asset_id(&asset.asset_id);
    }
    Err(SignerError::Other(format!("asset_resolution_failed:{raw}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::build_cat_ticker_index_lenient;
    use chia_sdk_coinset::CoinsetClient;

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
}
