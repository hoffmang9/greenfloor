//! Idempotent data backfills and normalizations (no table-shape changes).

use rusqlite::{params, Connection};

use super::super::db_err;
use super::super::offer_coin_watches::ensure_watch_rows;
use super::helpers::{query_mapped, rewritten_tx_id};
use crate::error::SignerResult;
use crate::hex::canonical_tx_id;

pub(super) fn backfill_offer_cancel_submitted_at(conn: &Connection) -> SignerResult<()> {
    conn.execute(
        r"
        UPDATE offer_state
        SET cancel_submitted_at = updated_at
        WHERE state = 'cancel_submitted'
          AND cancel_submitted_at IS NULL
        ",
        [],
    )
    .map_err(|err| db_err("backfill offer_state cancel_submitted_at", err))?;
    Ok(())
}

pub(super) fn normalize_legacy_tx_id_storage(conn: &Connection) -> SignerResult<()> {
    normalize_tx_signal_state_ids(conn)?;
    normalize_offer_cancel_submitted_tx_ids(conn)
}

/// Seed/heal watches + venue for pre-upgrade rows (one-shot via step table).
pub(super) fn backfill_watches_and_venue(conn: &Connection) -> SignerResult<()> {
    backfill_missing_offer_coin_watches(conn)?;
    backfill_offer_publish_venue(conn)
}

fn backfill_offer_publish_venue(conn: &Connection) -> SignerResult<()> {
    // Never infer `coinset` from 64-hex ids (Dexie `trade_id` shares that shape).
    // Leave 64-hex NULL unset (runtime treats non-`dexie` as Coinset-primary).
    // Label only unambiguous non-64-hex legacy ids as `dexie`.
    // Do not mass-clear explicit `coinset` — post-time writes are authoritative.
    let offer_ids: Vec<String> = query_mapped(
        conn,
        r"
        SELECT offer_id
        FROM offer_state
        WHERE publish_venue IS NULL OR length(trim(publish_venue)) = 0
        ",
        "publish_venue backfill",
        |row| row.get(0),
    )?;
    for offer_id in offer_ids {
        if canonical_tx_id(&offer_id).is_some() {
            continue;
        }
        conn.execute(
            "UPDATE offer_state SET publish_venue = 'dexie' WHERE offer_id = ?1",
            params![offer_id],
        )
        .map_err(|err| db_err(&format!("backfill publish_venue=dexie for {offer_id}"), err))?;
    }
    Ok(())
}

fn backfill_missing_offer_coin_watches(conn: &Connection) -> SignerResult<()> {
    // INSERT OR IGNORE heals both fully-missing and partial (coin-without-p2) rows.
    let rows: Vec<(String, String, Option<String>, Option<String>)> = query_mapped(
        conn,
        r"
        SELECT offer_id, market_id, cancel_input_coin_id, maker_puzzle_hash
        FROM offer_state
        WHERE state IN ('open', 'refresh_due', 'mempool_observed', 'pending_visibility')
          AND (
            (cancel_input_coin_id IS NOT NULL AND length(trim(cancel_input_coin_id)) > 0)
            OR (maker_puzzle_hash IS NOT NULL AND length(trim(maker_puzzle_hash)) > 0)
          )
        ",
        "offer_coin_watches backfill",
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    for (offer_id, market_id, input_coin, maker_p2) in rows {
        let coins: Vec<String> = input_coin.into_iter().collect();
        let p2s: Vec<String> = maker_p2.into_iter().collect();
        ensure_watch_rows(conn, &offer_id, &market_id, &coins, &p2s)?;
    }
    Ok(())
}

fn normalize_tx_signal_state_ids(conn: &Connection) -> SignerResult<()> {
    let legacy_rows: Vec<(String, String, Option<String>)> = query_mapped(
        conn,
        r"
        SELECT tx_id, mempool_observed_at, tx_block_confirmed_at
        FROM tx_signal_state
        ",
        "tx_signal_state migration",
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    for (raw_id, mempool, confirmed) in legacy_rows {
        let Some(canonical) = rewritten_tx_id(&raw_id) else {
            continue;
        };
        conn.execute(
            r"
            INSERT INTO tx_signal_state (tx_id, mempool_observed_at, tx_block_confirmed_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(tx_id) DO UPDATE SET
              mempool_observed_at = CASE
                WHEN excluded.mempool_observed_at < tx_signal_state.mempool_observed_at
                  THEN excluded.mempool_observed_at
                ELSE tx_signal_state.mempool_observed_at
              END,
              tx_block_confirmed_at = COALESCE(
                tx_signal_state.tx_block_confirmed_at,
                excluded.tx_block_confirmed_at
              )
            ",
            params![canonical, mempool, confirmed],
        )
        .map_err(|err| {
            db_err(
                &format!("migrate tx_signal_state id {raw_id} -> {canonical}"),
                err,
            )
        })?;
        conn.execute(
            "DELETE FROM tx_signal_state WHERE tx_id = ?1",
            params![raw_id],
        )
        .map_err(|err| db_err(&format!("delete legacy tx_signal_state id {raw_id}"), err))?;
    }
    Ok(())
}

fn normalize_offer_cancel_submitted_tx_ids(conn: &Connection) -> SignerResult<()> {
    let updates: Vec<(String, String)> = query_mapped(
        conn,
        r"
        SELECT offer_id, cancel_submitted_tx_id
        FROM offer_state
        WHERE cancel_submitted_tx_id IS NOT NULL
        ",
        "offer_state cancel tx migration",
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    for (offer_id, raw_id) in updates {
        let Some(canonical) = rewritten_tx_id(&raw_id) else {
            continue;
        };
        conn.execute(
            "UPDATE offer_state SET cancel_submitted_tx_id = ?1 WHERE offer_id = ?2",
            params![canonical, offer_id],
        )
        .map_err(|err| {
            db_err(
                &format!("migrate offer_state cancel_submitted_tx_id for {offer_id}"),
                err,
            )
        })?;
    }
    Ok(())
}
