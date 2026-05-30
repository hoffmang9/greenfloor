use crate::adapters::DexieClient;
use crate::cycle::{
    classify_dexie_stale_offer_status, collect_stale_sweep_candidates,
    is_dexie_offer_missing_error_text, record_stale_sweep_check, OfferStateRow, StaleSweepHit,
    StaleSweepProgress,
};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

const GLOBAL_STALE_OPEN_SWEEP_MAX_OFFERS_PER_MARKET: usize = 3;
const GLOBAL_STALE_OPEN_SWEEP_MAX_OFFER_CHECKS: usize = 60;

pub async fn detect_stale_open_offers_for_requeue(
    store: &SqliteStore,
    dexie: &DexieClient,
    enabled_market_ids: &[String],
) -> SignerResult<StaleSweepProgress> {
    if enabled_market_ids.is_empty() {
        return Ok(StaleSweepProgress {
            checked_offer_count: 0,
            requeue_market_ids: Vec::new(),
            hits: Vec::new(),
            truncated: false,
        });
    }

    let rows = store.list_offer_states(None, 5000)?;
    let offer_rows: Vec<OfferStateRow> = rows
        .into_iter()
        .map(|row| OfferStateRow {
            market_id: row.market_id,
            offer_id: row.offer_id,
            state: row.state,
        })
        .collect();
    let enabled_set: Vec<String> = enabled_market_ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    let candidates = collect_stale_sweep_candidates(
        &offer_rows,
        &enabled_set,
        GLOBAL_STALE_OPEN_SWEEP_MAX_OFFERS_PER_MARKET,
    );

    let mut progress = StaleSweepProgress {
        checked_offer_count: 0,
        requeue_market_ids: Vec::new(),
        hits: Vec::new(),
        truncated: false,
    };
    let check_limit = GLOBAL_STALE_OPEN_SWEEP_MAX_OFFER_CHECKS.max(1);

    for candidate in candidates {
        if progress.checked_offer_count >= check_limit {
            return Ok(StaleSweepProgress {
                truncated: true,
                ..progress
            });
        }
        let market_id = candidate.market_id.trim().to_string();
        let offer_id = candidate.offer_id.trim().to_string();
        let hit = match dexie.get_offer(&offer_id).await {
            Ok(payload) => {
                let offer = payload.get("offer");
                if let Some(offer_obj) = offer {
                    let status = offer_obj
                        .get("status")
                        .and_then(|value| value.as_i64())
                        .unwrap_or(-1);
                    classify_dexie_stale_offer_status(status).map(|reason| StaleSweepHit {
                        market_id: market_id.clone(),
                        offer_id: offer_id.clone(),
                        reason: reason.to_string(),
                    })
                } else {
                    None
                }
            }
            Err(err) if is_dexie_offer_missing_error_text(&err.to_string()) => {
                Some(StaleSweepHit {
                    market_id: market_id.clone(),
                    offer_id: offer_id.clone(),
                    reason: "offer_missing_404".to_string(),
                })
            }
            Err(_) => None,
        };
        progress = record_stale_sweep_check(&progress, hit);
    }

    Ok(progress)
}
