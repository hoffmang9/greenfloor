use serde_json::json;

use crate::coinset::{get_all_mempool_tx_ids, WsEvent};
use crate::config::ManagerProgramConfig;
use crate::daemon::coinset_ws::CoinsetWsShared;
use crate::error::SignerResult;
use crate::hex::normalize_hex_id;
use crate::offer::lifecycle::{
    apply_watch_hits_batch, apply_ws_offer_event, promote_cancel_submitted_for_confirmed_txs,
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
) -> SignerResult<()> {
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
    promote_cancel_submitted_for_confirmed_txs(store, confirmed_tx_ids)
}

fn record_observed_watch_keys(
    store: &SqliteStore,
    ctx: &CoinsetWsShared,
    observed_p2s: &[String],
    observed_coin_ids: &[String],
) -> SignerResult<()> {
    let inventory_markets = ctx.p2_index().market_ids_for_p2s(observed_p2s);
    for market_id in &inventory_markets {
        ctx.inventory_freshness.mark_stale(market_id);
    }
    let mut watch_keys: Vec<String> = observed_p2s
        .iter()
        .chain(observed_coin_ids.iter())
        .cloned()
        .collect();
    watch_keys.sort();
    watch_keys.dedup();
    let watch_markets = store.list_market_ids_for_watched_keys(&watch_keys)?;
    apply_watch_hits_batch(store, &watch_keys)?;
    if inventory_markets.is_empty() && watch_markets.is_empty() {
        return Ok(());
    }
    let mut sample: Vec<String> = watch_keys.iter().map(|key| normalize_hex_id(key)).collect();
    sample.sort();
    sample.truncate(10);
    let mut market_ids = inventory_markets;
    market_ids.extend(watch_markets);
    market_ids.sort();
    market_ids.dedup();
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

pub(crate) fn apply_ws_event(
    store: &SqliteStore,
    ctx: &CoinsetWsShared,
    event: WsEvent,
) -> SignerResult<()> {
    match event {
        WsEvent::Transaction(tx) => {
            match tx.status.as_str() {
                "pending" if !tx.tx_ids.is_empty() => {
                    record_ws_mempool_tx_ids(store, &tx.tx_ids)?;
                }
                "confirmed" if !tx.tx_ids.is_empty() => {
                    record_ws_confirmed_tx_ids(store, &tx.tx_ids)?;
                }
                _ => {}
            }
            if !tx.p2s.is_empty() || !tx.coin_ids.is_empty() {
                record_observed_watch_keys(store, ctx, &tx.p2s, &tx.coin_ids)?;
            }
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
            apply_ws_offer_event(store, &offer)?;
        }
    }
    Ok(())
}
