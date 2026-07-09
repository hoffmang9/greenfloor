//! Apply Coinset WS offer / watch signals through canonical reconcile decision.

use std::collections::HashMap;

use crate::coinset::WsOfferEvent;
use crate::cycle::reconcile::{
    signals_from_ws_offer_status, CancelSubmittedContext, CoinsetTxSignals,
};
use crate::error::SignerResult;
use crate::storage::{OfferStateListRow, SqliteStore, TxSignalIngress};

use super::cancel_context::{
    cancel_submitted_context_for_offer, preload_cancel_submitted_contexts,
};
use super::persist::ReconcilePersistOptions;
use super::signal_apply::apply_watched_offer_signals;

#[cfg(test)]
mod tests;

fn seed_offer_tx_signal(store: &SqliteStore, event: &WsOfferEvent) -> SignerResult<()> {
    let Some(tx_id) = event.tx_id.as_ref() else {
        return Ok(());
    };
    let kind = match event.status.as_str() {
        "confirmed" => TxSignalIngress::Confirmed,
        "pending" | "cancel_pending" => TxSignalIngress::Mempool,
        _ => return Ok(()),
    };
    store.ingest_tx_signals(std::slice::from_ref(tx_id), kind)?;
    Ok(())
}

fn ws_persist_options() -> ReconcilePersistOptions<'static> {
    ReconcilePersistOptions {
        action: "coinset_ws_lifecycle",
        venue: Some("coinset"),
        dexie_error: None,
    }
}

fn apply_row(
    store: &SqliteStore,
    row: &OfferStateListRow,
    status: Option<i64>,
    signals: CoinsetTxSignals,
    cancel_by_offer: Option<&HashMap<String, CancelSubmittedContext>>,
) -> SignerResult<()> {
    let cancel_submitted =
        cancel_submitted_context_for_offer(store, &row.offer_id, &row.state, cancel_by_offer)?;
    apply_watched_offer_signals(
        store,
        &row.market_id,
        &row.offer_id,
        &row.state,
        status,
        signals,
        cancel_submitted.as_ref(),
        &ws_persist_options(),
    )
}

/// Drive lifecycle from a Coinset WS `offer` event for a locally tracked offer.
///
/// # Errors
///
/// Returns an error if `SQLite` or reconcile persist fails.
pub fn apply_ws_offer_event(store: &SqliteStore, event: &WsOfferEvent) -> SignerResult<()> {
    seed_offer_tx_signal(store, event)?;
    let Some((status, signals)) =
        signals_from_ws_offer_status(&event.status, event.tx_id.as_deref())
    else {
        return Ok(());
    };
    let rows = store.list_offer_states_for_ids(std::slice::from_ref(&event.offer_id))?;
    let Some(row) = rows.first() else {
        return Ok(());
    };
    apply_row(store, row, status, signals, None)
}

/// Promote `cancel_submitted` offers whose cancel tx ids were just confirmed.
///
/// Caller must already have ingested the confirmed tx ids (`TxSignalIngress::Confirmed`)
/// so preloaded cancel context sees `tx_block_confirmed_at`.
///
/// # Errors
///
/// Returns an error if `SQLite` or reconcile persist fails.
pub fn promote_cancel_submitted_for_confirmed_txs(
    store: &SqliteStore,
    confirmed_tx_ids: &[String],
) -> SignerResult<()> {
    if confirmed_tx_ids.is_empty() {
        return Ok(());
    }
    let rows = store.list_offer_states_for_cancel_submitted_tx_ids(confirmed_tx_ids)?;
    if rows.is_empty() {
        return Ok(());
    }
    let cancel_by_offer = preload_cancel_submitted_contexts(store, &rows)?;
    // Do not wrap in a parent transaction: terminal persist uses
    // immediate_transaction (clear watches + upsert) and cannot nest.
    for row in &rows {
        apply_row(
            store,
            row,
            None,
            CoinsetTxSignals::default(),
            Some(&cancel_by_offer),
        )?;
    }
    Ok(())
}

/// On durable coin/p2 watch hits, mark `mempool_observed` via reconcile dispatch (batched).
///
/// Pure watch hits while `cancel_submitted` are preserved by cancel policy (not ignored here).
///
/// # Errors
///
/// Returns an error if `SQLite` or reconcile persist fails.
pub fn apply_watch_hits_batch(store: &SqliteStore, watched_keys: &[String]) -> SignerResult<()> {
    if watched_keys.is_empty() {
        return Ok(());
    }
    let rows = store.list_offer_states_for_watched_keys(watched_keys)?;
    if rows.is_empty() {
        return Ok(());
    }
    let cancel_by_offer = preload_cancel_submitted_contexts(store, &rows)?;
    store.unchecked_transaction_scope("watch_hits_batch", |store| {
        for row in &rows {
            apply_row(
                store,
                row,
                None,
                CoinsetTxSignals::watch_hit(),
                Some(&cancel_by_offer),
            )?;
        }
        Ok(())
    })
}
