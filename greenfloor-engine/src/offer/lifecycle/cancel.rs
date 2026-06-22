use crate::adapters::DexieClient;
use crate::config::SignerConfig;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::cancel_onchain::{cancel_offer_on_chain, CancelOfferOnChainParams};

#[derive(Debug, Clone)]
pub struct CancelOfferTarget {
    pub offer_id: String,
    pub market_id: String,
    pub receive_address: String,
}

#[derive(Debug, Clone)]
pub struct CancelOfferOutcome {
    pub offer_id: String,
    pub market_id: String,
    pub success: bool,
    pub operation_id: String,
    pub error: String,
}

/// Cancel offers on-chain (spend an offered input coin back to vault change).
///
/// Dexie is used only to fetch the offer file text; cancellation is submitted via Coinset.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn cancel_offers_on_chain(
    store: &SqliteStore,
    dexie: &DexieClient,
    signer_config: SignerConfig,
    targets: &[CancelOfferTarget],
) -> SignerResult<Vec<CancelOfferOutcome>> {
    let mut outcomes = Vec::with_capacity(targets.len());
    for target in targets {
        let market_id = if target.market_id.trim().is_empty() {
            "unknown".to_string()
        } else {
            target.market_id.clone()
        };
        match cancel_offer_on_chain(CancelOfferOnChainParams {
            offer_id: &target.offer_id,
            receive_address: &target.receive_address,
            signer_config: signer_config.clone(),
            dexie,
            fee_mojos: 0,
        })
        .await
        {
            Ok(result) => {
                store.upsert_offer_state(&target.offer_id, &market_id, "cancelled", Some(3))?;
                outcomes.push(CancelOfferOutcome {
                    offer_id: target.offer_id.clone(),
                    market_id,
                    success: true,
                    operation_id: result.operation_id,
                    error: String::new(),
                });
            }
            Err(err) => {
                outcomes.push(CancelOfferOutcome {
                    offer_id: target.offer_id.clone(),
                    market_id,
                    success: false,
                    operation_id: String::new(),
                    error: err.to_string(),
                });
            }
        }
    }
    Ok(outcomes)
}

/// Backward-compatible alias for daemon/manager call sites.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn cancel_offers_on_dexie(
    store: &SqliteStore,
    dexie: &DexieClient,
    signer_config: SignerConfig,
    targets: &[CancelOfferTarget],
) -> SignerResult<Vec<CancelOfferOutcome>> {
    cancel_offers_on_chain(store, dexie, signer_config, targets).await
}

#[cfg(test)]
mod tests {
    use super::CancelOfferTarget;

    #[test]
    fn cancel_target_carries_receive_address() {
        let target = CancelOfferTarget {
            offer_id: "offer-1".to_string(),
            market_id: "m1".to_string(),
            receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w"
                .to_string(),
        };
        assert!(!target.receive_address.is_empty());
    }
}
