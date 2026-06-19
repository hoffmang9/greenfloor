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
        let venue_response = match dexie.cancel_offer(&target.offer_id).await {
            Ok(value) => value,
            Err(err) => {
                outcomes.push(DexieCancelOutcome {
                    offer_id: target.offer_id.clone(),
                    market_id,
                    success: false,
                    venue_response: Value::Null,
                    error: err.to_string(),
                });
                continue;
            }
        };
        let success = venue_response.get("success").and_then(Value::as_bool) == Some(true);
        if success {
            store.upsert_offer_state(&target.offer_id, &market_id, "cancelled", Some(3))?;
        }
        let error = venue_response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        outcomes.push(DexieCancelOutcome {
            offer_id: target.offer_id.clone(),
            market_id,
            success,
            venue_response,
            error: if success { String::new() } else { error },
        });
    }
    Ok(outcomes)
}
