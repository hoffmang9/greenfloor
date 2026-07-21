use serde_json::json;

use crate::coinset::{get_all_mempool_tx_ids, WsEvent, WsTransactionEvent};
use crate::config::ManagerProgramConfig;
use crate::daemon::coinset_ws::CoinsetWsShared;
use crate::error::SignerResult;
use crate::hex::normalize_hex_id;
use crate::offer::lifecycle::{
    apply_watch_hits_batch, apply_ws_offer_event, promote_cancel_submitted_for_confirmed_txs,
    WsOfferApply,
};
use crate::operator_log::{
    LogContext, COINSET_WS_MEMPOOL_EVENT, COINSET_WS_RECOVERY_POLL, COINSET_WS_RECOVERY_POLL_ERROR,
    COINSET_WS_TX_BLOCK_EVENT, COIN_WATCH_HIT, MEMPOOL_OBSERVED, TX_BLOCK_CONFIRMED,
};
use crate::storage::{SqliteStore, TxSignalIngress};

pub async fn run_recovery_poll(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    coinset_base_url: &str,
    reason: &str,
) -> SignerResult<()> {
    let base_url = coinset_base_url.trim();
    let base_opt = if base_url.is_empty() {
        None
    } else {
        Some(base_url)
    };
    match get_all_mempool_tx_ids(&program.network, base_opt).await {
        Ok(tx_ids) => {
            let new_count = store.ingest_tx_signals(&tx_ids, TxSignalIngress::Mempool)?;
            LogContext::COINSET.audit(
                store,
                COINSET_WS_RECOVERY_POLL,
                &json!({"reason": reason, "tx_id_count": tx_ids.len()}),
                None,
            )?;
            if new_count > 0 {
                LogContext::COINSET.audit(
                    store,
                    MEMPOOL_OBSERVED,
                    &json!({"new_tx_ids": new_count, "source": "coinset_websocket"}),
                    None,
                )?;
            }
            Ok(())
        }
        Err(err) => {
            LogContext::COINSET.audit(
                store,
                COINSET_WS_RECOVERY_POLL_ERROR,
                &json!({"reason": reason, "error": err.to_string()}),
                None,
            )?;
            Err(err)
        }
    }
}

fn record_ws_mempool_tx_ids(store: &SqliteStore, mempool_tx_ids: &[String]) -> SignerResult<()> {
    let new_count = store.ingest_tx_signals(mempool_tx_ids, TxSignalIngress::Mempool)?;
    LogContext::COINSET.audit(
        store,
        COINSET_WS_MEMPOOL_EVENT,
        &json!({"tx_id_count": mempool_tx_ids.len()}),
        None,
    )?;
    if new_count > 0 {
        LogContext::COINSET.audit(
            store,
            MEMPOOL_OBSERVED,
            &json!({"new_tx_ids": new_count, "source": "coinset_websocket"}),
            None,
        )?;
    }
    Ok(())
}

fn record_ws_confirmed_tx_ids(
    store: &SqliteStore,
    confirmed_tx_ids: &[String],
) -> SignerResult<Vec<String>> {
    let confirmed = store.ingest_tx_signals(confirmed_tx_ids, TxSignalIngress::Confirmed)?;
    LogContext::COINSET.audit(
        store,
        COINSET_WS_TX_BLOCK_EVENT,
        &json!({"tx_id_count": confirmed_tx_ids.len(), "confirmed_count": confirmed}),
        None,
    )?;
    LogContext::COINSET.audit(
        store,
        TX_BLOCK_CONFIRMED,
        &json!({
            "tx_ids": confirmed_tx_ids,
            "confirmed_count": confirmed,
            "source": "coinset_websocket",
        }),
        None,
    )?;
    // Cancel confirmation spends maker coins / returns change — refresh inventory.
    promote_cancel_submitted_for_confirmed_txs(store, confirmed_tx_ids)
}

/// Markets whose spendable inventory may have changed for a transaction frame.
fn markets_to_invalidate_for_tx(
    ctx: &CoinsetWsShared,
    tx: &WsTransactionEvent,
    watch_markets: &[String],
    cancel_markets: &[String],
) -> Vec<String> {
    let mut market_ids = ctx.p2_index().market_ids_for_p2s(&tx.p2s);
    market_ids.extend(watch_markets.iter().cloned());
    market_ids.extend(cancel_markets.iter().cloned());
    market_ids.sort();
    market_ids.dedup();
    market_ids
}

/// Markets whose spendable inventory may have changed for an offer frame.
///
/// Offer-frame p2s are intentionally ignored; only terminal take/cancel statuses
/// on a locally tracked offer free maker coins.
fn markets_to_invalidate_for_offer(offer_status: &str, apply: &WsOfferApply) -> Vec<String> {
    if !matches!(offer_status, "confirmed" | "cancelled" | "expired") {
        return Vec::new();
    }
    match apply {
        WsOfferApply::Applied { market_id } => vec![market_id.clone()],
        WsOfferApply::SeedOnly | WsOfferApply::NotTracked => Vec::new(),
    }
}

fn dedupe_watch_keys(observed_p2s: &[String], observed_coin_ids: &[String]) -> Vec<String> {
    let mut watch_keys: Vec<String> = observed_p2s
        .iter()
        .chain(observed_coin_ids.iter())
        .cloned()
        .collect();
    watch_keys.sort();
    watch_keys.dedup();
    watch_keys
}

fn audit_coin_watch_hit(
    store: &SqliteStore,
    observed_p2s: &[String],
    observed_coin_ids: &[String],
    watch_keys: &[String],
    market_ids: &[String],
) -> SignerResult<()> {
    if market_ids.is_empty() {
        return Ok(());
    }
    let mut sample: Vec<String> = watch_keys.iter().map(|key| normalize_hex_id(key)).collect();
    sample.sort();
    sample.truncate(10);
    LogContext::COINSET.audit(
        store,
        COIN_WATCH_HIT,
        &json!({
            "p2_count": observed_p2s.len(),
            "coin_id_count": observed_coin_ids.len(),
            "keys_sample": sample,
            "market_ids": market_ids,
            "source": "coinset_websocket",
        }),
        None,
    )
}

fn apply_transaction_watch_hits(
    store: &SqliteStore,
    tx: &WsTransactionEvent,
    frame_confirmed: bool,
) -> SignerResult<Vec<String>> {
    let has_keys = !tx.p2s.is_empty() || !tx.coin_ids.is_empty();
    if !has_keys {
        return Ok(Vec::new());
    }
    let watch_keys = dedupe_watch_keys(&tx.p2s, &tx.coin_ids);
    let watch_markets = store.list_market_ids_for_watched_keys(&watch_keys)?;
    apply_watch_hits_batch(store, &watch_keys, frame_confirmed, &tx.tx_ids)?;
    audit_coin_watch_hit(store, &tx.p2s, &tx.coin_ids, &watch_keys, &watch_markets)?;
    Ok(watch_markets)
}

pub(crate) fn apply_ws_event(
    store: &SqliteStore,
    ctx: &CoinsetWsShared,
    event: WsEvent,
) -> SignerResult<()> {
    match event {
        WsEvent::Transaction(tx) => {
            let mut cancel_markets = Vec::new();
            // Allowlist only: unknown status marks inventory stale but must not
            // invent mempool_observed / tx_block_confirmed.
            let watch_markets = match tx.status.as_str() {
                "pending" => {
                    if !tx.tx_ids.is_empty() {
                        record_ws_mempool_tx_ids(store, &tx.tx_ids)?;
                    }
                    apply_transaction_watch_hits(store, &tx, false)?
                }
                "confirmed" => {
                    if !tx.tx_ids.is_empty() {
                        cancel_markets = record_ws_confirmed_tx_ids(store, &tx.tx_ids)?;
                    }
                    apply_transaction_watch_hits(store, &tx, true)?
                }
                _ => {
                    let watch_keys = dedupe_watch_keys(&tx.p2s, &tx.coin_ids);
                    if watch_keys.is_empty() {
                        Vec::new()
                    } else {
                        store.list_market_ids_for_watched_keys(&watch_keys)?
                    }
                }
            };
            let markets = markets_to_invalidate_for_tx(ctx, &tx, &watch_markets, &cancel_markets);
            ctx.inventory_freshness
                .mark_stale_markets(markets.iter().map(String::as_str));
        }
        WsEvent::Offer(offer) => {
            LogContext::COINSET.audit(
                store,
                "coinset_ws_offer_event",
                &json!({
                    "offer_id": offer.offer_id,
                    "status": offer.status,
                    "tx_id": offer.tx_id,
                    "p2_count": offer.p2s.len(),
                    "source": "coinset_websocket",
                }),
                None,
            )?;
            let apply = apply_ws_offer_event(store, &offer)?;
            let markets = markets_to_invalidate_for_offer(&offer.status, &apply);
            ctx.inventory_freshness
                .mark_stale_markets(markets.iter().map(String::as_str));
        }
    }
    Ok(())
}
