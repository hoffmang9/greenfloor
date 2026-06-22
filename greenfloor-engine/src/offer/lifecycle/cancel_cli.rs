use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::adapters::DexieClient;
use crate::config::{MarketConfig, SignerConfig};
use crate::error::{SignerError, SignerResult};
use crate::storage::{OfferStateListRow, SqliteStore};

use super::cancel::{cancel_offers_on_chain, CancelOfferTarget};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffersCancelCliItem {
    pub offer_id: String,
    pub market_id: String,
    pub state: String,
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffersCancelCliResult {
    pub venue: String,
    pub cancel_open: bool,
    pub requested_offer_ids: Vec<String>,
    pub selected_count: u64,
    pub cancelled_count: u64,
    pub failed_count: u64,
    pub items: Vec<OffersCancelCliItem>,
}

#[derive(Debug, Clone)]
struct SelectedOffer {
    offer_id: String,
    market_id: String,
    state: String,
}

fn select_offers_for_cancel(
    rows: &[OfferStateListRow],
    offer_ids: &[String],
    cancel_open: bool,
) -> SignerResult<Vec<SelectedOffer>> {
    let normalized = rows
        .iter()
        .filter_map(|row| {
            let offer_id = row.offer_id.trim();
            if offer_id.is_empty() {
                return None;
            }
            Some(SelectedOffer {
                offer_id: offer_id.to_string(),
                market_id: row.market_id.trim().to_string(),
                state: row.state.trim().to_ascii_lowercase(),
            })
        })
        .collect::<Vec<_>>();
    if cancel_open {
        return Ok(normalized
            .into_iter()
            .filter(|row| row.state == "open" || row.state == "pending_visibility")
            .collect());
    }
    let requested_ids: std::collections::HashSet<String> = offer_ids
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if requested_ids.is_empty() {
        return Err(SignerError::Other(
            "provide at least one --offer-id or pass --cancel-open".to_string(),
        ));
    }
    Ok(normalized
        .into_iter()
        .filter(|row| requested_ids.contains(&row.offer_id))
        .collect())
}

fn load_rows_for_cancel(
    store: &SqliteStore,
    offer_ids: &[String],
    cancel_open: bool,
) -> SignerResult<Vec<OfferStateListRow>> {
    if cancel_open {
        return store.list_open_offer_states(10_000);
    }
    store.list_offer_states_for_ids(offer_ids)
}

fn receive_address_for_market(
    market_by_id: &HashMap<String, MarketConfig>,
    market_id: &str,
) -> SignerResult<String> {
    let market = market_by_id
        .get(market_id)
        .ok_or_else(|| SignerError::Other(format!("unknown market_id for cancel: {market_id}")))?;
    let receive_address = market.receive_address.trim();
    if receive_address.is_empty() {
        return Err(SignerError::Other(format!(
            "missing receive_address for market {market_id}"
        )));
    }
    Ok(receive_address.to_string())
}

/// Offers cancel cli.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn offers_cancel_cli(
    db_path: &Path,
    dexie_base_url: &str,
    target_venue: &str,
    offer_ids: &[String],
    cancel_open: bool,
    signer_config: SignerConfig,
    market_by_id: &HashMap<String, MarketConfig>,
) -> SignerResult<OffersCancelCliResult> {
    let venue = target_venue.trim().to_ascii_lowercase();
    if venue != "dexie" {
        return Err(SignerError::Other(format!(
            "offer cancel supports dexie venue only (got {venue})"
        )));
    }
    let store = SqliteStore::open(db_path)?;
    let dexie = DexieClient::new(dexie_base_url);
    let requested_offer_ids: Vec<String> = offer_ids
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    let rows = load_rows_for_cancel(&store, &requested_offer_ids, cancel_open)?;
    let selected = select_offers_for_cancel(&rows, &requested_offer_ids, cancel_open)?;
    let targets: Vec<CancelOfferTarget> = selected
        .iter()
        .map(|row| {
            Ok(CancelOfferTarget {
                offer_id: row.offer_id.clone(),
                market_id: row.market_id.clone(),
                receive_address: receive_address_for_market(market_by_id, &row.market_id)?,
            })
        })
        .collect::<SignerResult<_>>()?;
    let outcomes = cancel_offers_on_chain(&store, &dexie, signer_config, &targets).await?;
    let mut items = Vec::with_capacity(outcomes.len());
    let mut failures = 0u64;
    for (outcome, row) in outcomes.into_iter().zip(selected) {
        if !outcome.success {
            failures += 1;
        }
        items.push(OffersCancelCliItem {
            offer_id: row.offer_id,
            market_id: row.market_id,
            state: row.state,
            result: json!({
                "success": outcome.success,
                "operation_id": outcome.operation_id,
                "error": outcome.error,
            }),
        });
    }
    let selected_count = crate::metrics::metric_collection_len_to_u64(items.len());
    Ok(OffersCancelCliResult {
        venue,
        cancel_open,
        requested_offer_ids,
        selected_count,
        cancelled_count: selected_count.saturating_sub(failures),
        failed_count: failures,
        items,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn load_rows_for_cancel_by_id_finds_old_offer() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        for idx in 0..600 {
            store
                .upsert_offer_state(
                    &format!("offer-{idx}"),
                    "m1",
                    if idx == 0 { "open" } else { "expired" },
                    Some(0),
                )
                .expect("seed");
        }
        store
            .upsert_offer_state("old-offer", "m1", "open", Some(0))
            .expect("seed old");
        let rows = load_rows_for_cancel(&store, &["old-offer".to_string()], false).expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].offer_id, "old-offer");
    }

    #[tokio::test]
    async fn cancel_cli_cancel_open_updates_state() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("offer-open", "m1", "open", Some(0))
            .expect("seed");
        store
            .upsert_offer_state("offer-expired", "m1", "expired", Some(0))
            .expect("seed");

        // On-chain cancel requires vault/KMS; this test only verifies selection wiring.
        let rows = load_rows_for_cancel(&store, &[], true).expect("rows");
        let selected = select_offers_for_cancel(&rows, &[], true).expect("selected");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].offer_id, "offer-open");
    }

    #[tokio::test]
    async fn cancel_cli_reports_missing_market_receive_address() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("offer-target", "m1", "open", Some(0))
            .expect("seed");
        let rows =
            load_rows_for_cancel(&store, &["offer-target".to_string()], false).expect("rows");
        let selected = select_offers_for_cancel(&rows, &["offer-target".to_string()], false)
            .expect("selected");
        let err = receive_address_for_market(&HashMap::new(), &selected[0].market_id).unwrap_err();
        assert!(err.to_string().contains("unknown market_id"));
    }
}
