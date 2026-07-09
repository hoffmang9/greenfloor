use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::adapters::DexieClient;
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::storage::{OfferStateListRow, SqliteStore};

use super::cancel::{
    cancel_offers_on_chain, cancel_targets_need_dexie_fallback, CancelOfferTarget,
};
use super::cancel_context::{
    cancel_submit_in_flight_skip_result, partition_defer_in_flight_cancel_targets,
};
use super::cancel_eligibility::filter_cancel_target_offer_ids;

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
    pub skipped_count: u64,
    pub submitted_count: u64,
    pub failed_count: u64,
    pub items: Vec<OffersCancelCliItem>,
}

#[derive(Debug, Clone)]
struct CancelCliSelection {
    target: CancelOfferTarget,
    state: String,
}

fn fallback_market_id(default_market_id: Option<&str>) -> String {
    default_market_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

fn select_targets_for_cancel(
    rows: &[OfferStateListRow],
    offer_ids: &[String],
    cancel_open: bool,
    default_market_id: Option<&str>,
) -> SignerResult<Vec<CancelCliSelection>> {
    if cancel_open {
        // Same venue gating as daemon: empty Dexie status map excludes Dexie-venue
        // offers (operator override is explicit `--offer-id`).
        let target_ids = filter_cancel_target_offer_ids(rows, &HashMap::new());
        let row_by_id: HashMap<&str, &OfferStateListRow> = rows
            .iter()
            .map(|row| (row.offer_id.as_str(), row))
            .collect();
        return Ok(target_ids
            .into_iter()
            .filter_map(|offer_id| {
                let row = row_by_id.get(offer_id.as_str())?;
                Some(CancelCliSelection {
                    target: CancelOfferTarget::Tracked {
                        offer_id,
                        market_id: row.market_id.trim().to_string(),
                    },
                    state: row.state.trim().to_ascii_lowercase(),
                })
            })
            .collect());
    }
    let requested_ids: Vec<String> = offer_ids
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if requested_ids.is_empty() {
        return Err(SignerError::Other(
            "provide at least one --offer-id, --offer-file, or pass --cancel-open".to_string(),
        ));
    }
    let row_by_id: HashMap<String, &OfferStateListRow> =
        rows.iter().map(|row| (row.offer_id.clone(), row)).collect();
    let fallback_market_id = fallback_market_id(default_market_id);
    Ok(requested_ids
        .into_iter()
        .map(|offer_id| {
            if let Some(row) = row_by_id.get(&offer_id) {
                CancelCliSelection {
                    target: CancelOfferTarget::Tracked {
                        offer_id: offer_id.clone(),
                        market_id: row.market_id.trim().to_string(),
                    },
                    state: row.state.trim().to_ascii_lowercase(),
                }
            } else {
                CancelCliSelection {
                    target: CancelOfferTarget::Tracked {
                        offer_id,
                        market_id: fallback_market_id.clone(),
                    },
                    state: "unknown".to_string(),
                }
            }
        })
        .collect())
}

fn load_rows_for_cancel(
    store: &SqliteStore,
    offer_ids: &[String],
    cancel_open: bool,
) -> SignerResult<Vec<OfferStateListRow>> {
    if cancel_open {
        return store.list_all_open_offer_states();
    }
    store.list_offer_states_for_ids(offer_ids)
}

fn load_offer_file_text(value: &str) -> SignerResult<String> {
    let trimmed = value.trim();
    if trimmed.starts_with("offer1") {
        return Ok(trimmed.to_string());
    }
    let path = Path::new(trimmed);
    if path.exists() {
        return std::fs::read_to_string(path).map_err(|err| {
            SignerError::Other(format!(
                "failed to read offer file {}: {err}",
                path.display()
            ))
        });
    }
    Err(SignerError::Other(format!(
        "offer file not found and value is not offer1 bech32: {trimmed}"
    )))
}

fn offer_id_for_file(path_or_bech32: &str, idx: usize) -> String {
    let trimmed = path_or_bech32.trim();
    if trimmed.starts_with("offer1") {
        return format!("local-offer-{}", &trimmed[6..trimmed.len().min(22)]);
    }
    Path::new(trimmed)
        .file_stem()
        .and_then(|value| value.to_str())
        .map_or_else(|| format!("local-offer-{idx}"), str::to_string)
}

fn targets_from_offer_files(
    offer_files: &[String],
    default_market_id: Option<&str>,
) -> SignerResult<Vec<CancelCliSelection>> {
    let market_id = fallback_market_id(default_market_id);
    offer_files
        .iter()
        .enumerate()
        .map(|(idx, path_or_bech32)| {
            let offer_text = load_offer_file_text(path_or_bech32)?;
            Ok(CancelCliSelection {
                target: CancelOfferTarget::LocalFile {
                    offer_id: offer_id_for_file(path_or_bech32, idx),
                    market_id: market_id.clone(),
                    offer_text,
                },
                state: "local".to_string(),
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct OffersCancelCliRequest {
    pub offer_ids: Vec<String>,
    pub offer_files: Vec<String>,
    pub market_id: Option<String>,
    pub cancel_open: bool,
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
    request: &OffersCancelCliRequest,
    signer_config: SignerConfig,
    operator_network: &str,
) -> SignerResult<OffersCancelCliResult> {
    let venue = normalize_cancel_venue(target_venue)?;
    validate_cancel_cli_request(request)?;
    let store = SqliteStore::open(db_path)?;
    let requested_offer_ids: Vec<String> = request
        .offer_ids
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if !request.cancel_open && requested_offer_ids.is_empty() && request.offer_files.is_empty() {
        return Err(SignerError::Other(
            "provide at least one --offer-id, --offer-file, or pass --cancel-open".to_string(),
        ));
    }
    let (mut selections, skipped_items, skipped_count) =
        collect_cancel_cli_selections(&store, request, &requested_offer_ids)?;
    let mut items = skipped_items;
    let targets: Vec<CancelOfferTarget> = selections
        .iter()
        .map(|selection| selection.target.clone())
        .collect();
    let dexie = if cancel_targets_need_dexie_fallback(&store, &targets)? {
        Some(DexieClient::new(dexie_base_url))
    } else {
        None
    };
    let outcomes = cancel_offers_on_chain(
        &store,
        dexie.as_ref(),
        signer_config,
        operator_network,
        &targets,
    )
    .await?;
    let selected_count = crate::metrics::metric_collection_len_to_u64(selections.len());
    let mut failures = 0u64;
    for (outcome, selection) in outcomes.into_iter().zip(selections.drain(..)) {
        if !outcome.success {
            failures += 1;
        }
        items.push(OffersCancelCliItem {
            offer_id: selection.target.offer_id().to_string(),
            market_id: selection.target.normalized_market_id(),
            state: selection.state,
            result: json!({
                "success": outcome.success,
                "operation_id": outcome.operation_id,
                "error": outcome.error,
                "warning": outcome.warning,
            }),
        });
    }
    Ok(OffersCancelCliResult {
        venue,
        cancel_open: request.cancel_open,
        requested_offer_ids,
        selected_count,
        skipped_count,
        submitted_count: selected_count.saturating_sub(failures),
        failed_count: failures,
        items,
    })
}

fn normalize_cancel_venue(target_venue: &str) -> SignerResult<String> {
    let venue = target_venue.trim().to_ascii_lowercase();
    if venue != "dexie" && venue != "coinset" && venue != "splash" {
        return Err(SignerError::Other(format!(
            "offer cancel supports coinset|dexie|splash (got {venue})"
        )));
    }
    Ok(venue)
}

fn validate_cancel_cli_request(request: &OffersCancelCliRequest) -> SignerResult<()> {
    if request.cancel_open && !request.offer_files.is_empty() {
        return Err(SignerError::Other(
            "--cancel-open cannot be combined with --offer-file".to_string(),
        ));
    }
    Ok(())
}

fn collect_cancel_cli_selections(
    store: &SqliteStore,
    request: &OffersCancelCliRequest,
    requested_offer_ids: &[String],
) -> SignerResult<(Vec<CancelCliSelection>, Vec<OffersCancelCliItem>, u64)> {
    let rows = load_rows_for_cancel(store, requested_offer_ids, request.cancel_open)?;
    let mut selections = select_targets_for_cancel(
        &rows,
        requested_offer_ids,
        request.cancel_open,
        request.market_id.as_deref(),
    )?;
    selections.extend(targets_from_offer_files(
        &request.offer_files,
        request.market_id.as_deref(),
    )?);
    let partition = partition_defer_in_flight_cancel_targets(
        store,
        &rows,
        selections,
        Utc::now(),
        |selection| selection.target.offer_id(),
        |selection| selection.target.persists_state(),
    )?;
    let skipped_count = crate::metrics::metric_collection_len_to_u64(partition.skipped.len());
    let skipped_items = partition
        .skipped
        .iter()
        .map(|selection| OffersCancelCliItem {
            offer_id: selection.target.offer_id().to_string(),
            market_id: selection.target.normalized_market_id(),
            state: selection.state.clone(),
            result: cancel_submit_in_flight_skip_result(),
        })
        .collect();
    Ok((partition.active, skipped_items, skipped_count))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::offer::lifecycle::cancel_context::{
        cancel_submit_in_flight_skip_result, partition_defer_in_flight_cancel_targets,
    };

    #[test]
    fn load_rows_for_cancel_open_includes_stale_offer_beyond_recency_window() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("aaa-stale-offer", "m1", "open", Some(0))
            .expect("seed stale");
        for idx in 0..1_200 {
            store
                .upsert_offer_state(&format!("zzz-offer-{idx:04}"), "m1", "open", Some(0))
                .expect("seed recent");
        }
        let rows = load_rows_for_cancel(&store, &[], true).expect("rows");
        assert!(
            rows.iter().any(|row| row.offer_id == "aaa-stale-offer"),
            "cancel-open must include lexically early open offers, not only recently updated rows"
        );
    }

    #[test]
    fn cancel_open_skips_dexie_venue_without_status_map() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state_with_metadata_at(
                "offer-coinset",
                "m1",
                "open",
                Some(0),
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: Some("coinset"),
                    ..Default::default()
                },
            )
            .expect("seed coinset");
        store
            .upsert_offer_state_with_metadata_at(
                "offer-dexie",
                "m1",
                "open",
                Some(0),
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: Some("dexie"),
                    ..Default::default()
                },
            )
            .expect("seed dexie");
        let rows = load_rows_for_cancel(&store, &[], true).expect("rows");
        let selected = select_targets_for_cancel(&rows, &[], true, None).expect("selected");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].target.offer_id(), "offer-coinset");
    }

    #[test]
    fn cancel_open_skips_cancel_submitted_offers() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("offer-open", "m1", "open", Some(0))
            .expect("seed");
        store
            .upsert_offer_state("offer-cancel-submitted", "m1", "cancel_submitted", None)
            .expect("seed");
        let rows = load_rows_for_cancel(&store, &[], true).expect("rows");
        let selected = select_targets_for_cancel(&rows, &[], true, None).expect("selected");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].target.offer_id(), "offer-open");
    }

    #[test]
    fn explicit_offer_id_skips_in_flight_cancel_submitted() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        let tx_id = "b".repeat(64);
        store
            .upsert_offer_cancel_submitted("offer-cancel-submitted", "m1", &tx_id, Some(0))
            .expect("seed cancel submitted");
        let rows = load_rows_for_cancel(&store, &["offer-cancel-submitted".to_string()], false)
            .expect("rows");
        let selected =
            select_targets_for_cancel(&rows, &["offer-cancel-submitted".to_string()], false, None)
                .expect("selected");
        assert_eq!(selected.len(), 1);
        let partition = partition_defer_in_flight_cancel_targets(
            &store,
            &rows,
            selected,
            Utc::now(),
            |selection| selection.target.offer_id(),
            |selection| selection.target.persists_state(),
        )
        .expect("partition");
        assert!(partition.active.is_empty());
        assert_eq!(partition.skipped.len(), 1);
        assert_eq!(
            cancel_submit_in_flight_skip_result().get("skipped"),
            Some(&json!(true))
        );
        assert_eq!(
            cancel_submit_in_flight_skip_result().get("reason"),
            Some(&json!("cancel_submit_in_flight"))
        );
    }

    #[test]
    fn select_targets_for_cancel_includes_requested_id_missing_from_db() {
        let selected =
            select_targets_for_cancel(&[], &["dexie-only-offer".to_string()], false, Some("m1"))
                .expect("selected");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].target.offer_id(), "dexie-only-offer");
        assert_eq!(selected[0].target.market_id(), "m1");
        assert_eq!(selected[0].state, "unknown");
    }

    #[test]
    fn targets_from_bech32_offer_file() {
        let offer = "offer1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";
        let selected =
            targets_from_offer_files(&[offer.to_string()], Some("m1")).expect("selected");
        assert_eq!(selected.len(), 1);
        assert!(selected[0].target.offer_text().is_some());
        assert_eq!(selected[0].target.market_id(), "m1");
        assert!(!selected[0].target.persists_state());
    }

    #[test]
    fn load_offer_file_text_reads_path() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("offer.txt");
        std::fs::write(&path, "offer1fromdisk").expect("write");
        let text = load_offer_file_text(path.to_str().expect("path")).expect("text");
        assert_eq!(text, "offer1fromdisk");
    }
}
