use serde_json::Value;

use crate::adapters::DexieClient;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

#[derive(Debug, Clone)]
pub struct CancelOfferTarget {
    pub offer_id: String,
    pub market_id: String,
}

#[derive(Debug, Clone)]
pub struct DexieCancelOutcome {
    pub offer_id: String,
    pub market_id: String,
    pub success: bool,
    pub venue_response: Value,
    pub error: String,
}

/// Cancel offers on dexie.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn cancel_offers_on_dexie(
    store: &SqliteStore,
    dexie: &DexieClient,
    targets: &[CancelOfferTarget],
) -> SignerResult<Vec<DexieCancelOutcome>> {
    let mut outcomes = Vec::with_capacity(targets.len());
    for target in targets {
        let market_id = if target.market_id.trim().is_empty() {
            "unknown".to_string()
        } else {
            target.market_id.clone()
        };
        match dexie.cancel_offer(&target.offer_id).await {
            Ok(response) => {
                let success = response.success();
                if success {
                    store.upsert_offer_state(&target.offer_id, &market_id, "cancelled", Some(3))?;
                }
                let error = response.error_text().to_string();
                outcomes.push(DexieCancelOutcome {
                    offer_id: target.offer_id.clone(),
                    market_id,
                    success,
                    venue_response: response.into_value(),
                    error: if success { String::new() } else { error },
                });
            }
            Err(err) => {
                outcomes.push(DexieCancelOutcome {
                    offer_id: target.offer_id.clone(),
                    market_id,
                    success: false,
                    venue_response: Value::Null,
                    error: err.to_string(),
                });
            }
        }
    }
    Ok(outcomes)
}
