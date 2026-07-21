//! Apply Coinset WS offer / watch signals through canonical reconcile decision.

use chrono::Utc;

use crate::coinset::WsOfferEvent;
use crate::config::Venue;
use crate::cycle::reconcile::{signals_from_ws_offer_status, CoinsetTxSignals};
use crate::error::SignerResult;
use crate::storage::{SqliteStore, TxSignalIngress};

use super::cancel_context::preload_cancel_submitted_contexts;
use super::persist::ReconcilePersistOptions;
use super::signal_apply::{apply_cancel_submitted_rows, apply_signals_to_row};

#[cfg(test)]
mod tests;

/// Result of applying a Coinset WS `offer` event to local state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsOfferApply {
    /// Local row found and lifecycle dispatch ran.
    Applied { market_id: String },
    /// Status only seeds `tx_signal_state` (`pending` / `cancel_pending`).
    SeedOnly,
    /// No local offer row for this id.
    NotTracked,
}

fn seed_offer_tx_signal(store: &SqliteStore, event: &WsOfferEvent) -> SignerResult<()> {
    let Some(tx_id) = event.tx_id.as_ref() else {
        return Ok(());
    };
    let kind = match event.status.as_str() {
        "confirmed" => TxSignalIngress::Confirmed,
        // Offer-frame pending / cancel_pending seed tx_signal_state only; they do
        // not drive mempool_observed (see signals_from_ws_offer_status).
        "pending" | "cancel_pending" => TxSignalIngress::Mempool,
        _ => return Ok(()),
    };
    store.ingest_tx_signals(std::slice::from_ref(tx_id), kind)?;
    Ok(())
}

fn ws_persist_options() -> ReconcilePersistOptions<'static> {
    ReconcilePersistOptions {
        action: "coinset_ws_lifecycle",
        venue: Some(Venue::Coinset),
        dexie_error: None,
    }
}

/// Signals for a durable maker coin watch hit.
///
/// Confirmed frames promote even when `confirmed_tx_ids` is empty (Coinset may
/// omit spend-bundle ids while still listing maker coin removals).
#[must_use]
pub fn signals_for_watch_hit(
    frame_confirmed: bool,
    confirmed_tx_ids: &[String],
) -> CoinsetTxSignals {
    if frame_confirmed {
        CoinsetTxSignals::confirmed_watch(confirmed_tx_ids)
    } else {
        CoinsetTxSignals::watch_hit()
    }
}

/// Drive lifecycle from a Coinset WS `offer` event for a locally tracked offer.
///
/// # Errors
///
/// Returns an error if `SQLite` or reconcile persist fails.
pub fn apply_ws_offer_event(
    store: &SqliteStore,
    event: &WsOfferEvent,
) -> SignerResult<WsOfferApply> {
    seed_offer_tx_signal(store, event)?;
    let Some((status, signals)) =
        signals_from_ws_offer_status(&event.status, event.tx_id.as_deref())
    else {
        return Ok(WsOfferApply::SeedOnly);
    };
    let rows = store.list_offer_states_for_ids(std::slice::from_ref(&event.offer_id))?;
    let Some(row) = rows.first() else {
        return Ok(WsOfferApply::NotTracked);
    };
    apply_signals_to_row(
        store,
        row,
        status,
        signals,
        None,
        &ws_persist_options(),
        Utc::now(),
    )?;
    Ok(WsOfferApply::Applied {
        market_id: row.market_id.clone(),
    })
}

/// Promote `cancel_submitted` offers whose cancel tx ids were just confirmed.
///
/// Returns unique `market_id`s for rows that were considered (for inventory stale).
/// Caller must already have ingested the confirmed tx ids (`TxSignalIngress::Confirmed`)
/// so preloaded cancel context sees `tx_block_confirmed_at`.
///
/// # Errors
///
/// Returns an error if `SQLite` or reconcile persist fails.
pub fn promote_cancel_submitted_for_confirmed_txs(
    store: &SqliteStore,
    confirmed_tx_ids: &[String],
) -> SignerResult<Vec<String>> {
    if confirmed_tx_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = store.list_offer_states_for_cancel_submitted_tx_ids(confirmed_tx_ids)?;
    // Do not wrap in a parent transaction: terminal persist uses
    // immediate_transaction (clear watches + upsert) and cannot nest.
    apply_cancel_submitted_rows(store, &rows, &ws_persist_options(), Utc::now())?;
    let mut market_ids: Vec<String> = rows.into_iter().map(|row| row.market_id).collect();
    market_ids.sort();
    market_ids.dedup();
    Ok(market_ids)
}

/// Apply durable coin watch hits through reconcile dispatch (batched).
///
/// Pending coin matches → [`CoinsetTxSignals::watch_hit`]. Confirmed coin matches
/// → [`CoinsetTxSignals::confirmed_watch`]. P2-only matches do not drive lifecycle,
/// but their markets are included in the returned inventory invalidation set.
///
/// Do not wrap in a parent transaction: terminal persist uses
/// `immediate_transaction` (clear watches + upsert) and cannot nest.
///
/// # Errors
///
/// Returns an error if `SQLite` or reconcile persist fails.
pub fn apply_watch_hits_batch(
    store: &SqliteStore,
    watched_keys: &[String],
    frame_confirmed: bool,
    confirmed_tx_ids: &[String],
) -> SignerResult<Vec<String>> {
    if watched_keys.is_empty() {
        return Ok(Vec::new());
    }
    let hits = store.match_watch_keys(watched_keys)?;
    let mut market_ids: Vec<String> = hits.iter().map(|hit| hit.row.market_id.clone()).collect();
    market_ids.sort();
    market_ids.dedup();
    let coin_hits: Vec<_> = hits.iter().filter(|hit| hit.kind.includes_coin()).collect();
    if coin_hits.is_empty() {
        return Ok(market_ids);
    }
    let rows: Vec<_> = coin_hits.iter().map(|hit| hit.row.clone()).collect();
    let cancel_by_offer = preload_cancel_submitted_contexts(store, &rows)?;
    let options = ws_persist_options();
    let now = Utc::now();
    for hit in coin_hits {
        let signals = signals_for_watch_hit(frame_confirmed, confirmed_tx_ids);
        apply_signals_to_row(
            store,
            &hit.row,
            None,
            signals,
            Some(&cancel_by_offer),
            &options,
            now,
        )?;
    }
    Ok(market_ids)
}
