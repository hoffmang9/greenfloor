use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{SignerError, SignerResult};
use crate::hex::canonical_tx_id;

fn column_exists(conn: &Connection, table: &str, column: &str) -> SignerResult<bool> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&sql).map_err(|err| {
        SignerError::Other(format!("failed to prepare table_info for {table}: {err}"))
    })?;
    let mut rows = stmt.query([]).map_err(|err| {
        SignerError::Other(format!("failed to query table_info for {table}: {err}"))
    })?;
    while let Some(row) = rows.next().map_err(|err| {
        SignerError::Other(format!("failed to read table_info row for {table}: {err}"))
    })? {
        let name: String = row
            .get(1)
            .map_err(|err| SignerError::Other(format!("failed to read column name: {err}")))?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> SignerResult<()> {
    if column_exists(conn, table, column)? {
        return Ok(());
    }
    let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
    conn.execute(&sql, []).map_err(|err| {
        SignerError::Other(format!("failed to add column {table}.{column}: {err}"))
    })?;
    Ok(())
}

const SCHEMA_META_WATCH_VENUE_BACKFILL: &str = "watch_venue_backfill_v2";

/// Apply additive schema migrations after base `CREATE TABLE IF NOT EXISTS` bootstrap.
///
/// # Errors
///
/// Returns an error if a migration fails for reasons other than idempotent re-run.
pub(crate) fn apply_schema_migrations(conn: &Connection) -> SignerResult<()> {
    add_column_if_missing(conn, "offer_state", "presplit_input_coin_id", "TEXT NULL")?;
    add_column_if_missing(
        conn,
        "offer_state",
        "fixed_delegated_puzzle_hash",
        "TEXT NULL",
    )?;
    add_column_if_missing(conn, "offer_state", "maker_puzzle_hash", "TEXT NULL")?;
    add_column_if_missing(conn, "offer_state", "execution_mode", "TEXT NULL")?;
    add_column_if_missing(conn, "offer_state", "cancel_submitted_tx_id", "TEXT NULL")?;
    add_column_if_missing(conn, "offer_state", "cancel_submitted_at", "TEXT NULL")?;
    add_column_if_missing(conn, "offer_state", "publish_venue", "TEXT NULL")?;
    ensure_offer_coin_watches_table(conn)?;
    ensure_schema_meta_table(conn)?;
    backfill_offer_cancel_submitted_at(conn)?;
    normalize_legacy_tx_id_storage(conn)?;
    // One-shot upgrade: seed/heal watches + venue for pre-upgrade rows only.
    run_watch_venue_backfill_once(conn)?;
    Ok(())
}

fn ensure_schema_meta_table(conn: &Connection) -> SignerResult<()> {
    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS schema_meta (
          key TEXT PRIMARY KEY,
          applied_at TEXT NOT NULL
        );
        ",
    )
    .map_err(|err| SignerError::Other(format!("schema_meta migrate: {err}")))?;
    Ok(())
}

fn schema_meta_applied(conn: &Connection, key: &str) -> SignerResult<bool> {
    let found: Option<String> = conn
        .query_row(
            "SELECT key FROM schema_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| {
            SignerError::Other(format!("failed to read schema_meta key {key}: {err}"))
        })?;
    Ok(found.is_some())
}

fn mark_schema_meta_applied(conn: &Connection, key: &str) -> SignerResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR IGNORE INTO schema_meta (key, applied_at) VALUES (?1, ?2)",
        params![key, now],
    )
    .map_err(|err| SignerError::Other(format!("failed to mark schema_meta key {key}: {err}")))?;
    Ok(())
}

fn run_watch_venue_backfill_once(conn: &Connection) -> SignerResult<()> {
    if schema_meta_applied(conn, SCHEMA_META_WATCH_VENUE_BACKFILL)? {
        return Ok(());
    }
    backfill_missing_offer_coin_watches(conn)?;
    backfill_offer_publish_venue(conn)?;
    mark_schema_meta_applied(conn, SCHEMA_META_WATCH_VENUE_BACKFILL)?;
    Ok(())
}

fn backfill_offer_publish_venue(conn: &Connection) -> SignerResult<()> {
    // Never infer `coinset` from 64-hex ids (Dexie `trade_id` shares that shape).
    // Leave 64-hex NULL unset (runtime treats non-`dexie` as Coinset-primary).
    // Label only unambiguous non-64-hex legacy ids as `dexie`.
    // Do not mass-clear explicit `coinset` — post-time writes are authoritative.
    let mut stmt = conn
        .prepare(
            r"
            SELECT offer_id
            FROM offer_state
            WHERE publish_venue IS NULL OR length(trim(publish_venue)) = 0
            ",
        )
        .map_err(|err| {
            SignerError::Other(format!(
                "failed to prepare publish_venue backfill query: {err}"
            ))
        })?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| {
            SignerError::Other(format!(
                "failed to query publish_venue backfill rows: {err}"
            ))
        })?;
    for row in rows {
        let offer_id = row.map_err(|err| {
            SignerError::Other(format!("failed to read publish_venue backfill row: {err}"))
        })?;
        if crate::hex::normalize_hex_id(&offer_id).len() == 64 {
            continue;
        }
        conn.execute(
            "UPDATE offer_state SET publish_venue = 'dexie' WHERE offer_id = ?1",
            params![offer_id],
        )
        .map_err(|err| {
            SignerError::Other(format!(
                "failed to backfill publish_venue=dexie for {offer_id}: {err}"
            ))
        })?;
    }
    Ok(())
}

fn backfill_missing_offer_coin_watches(conn: &Connection) -> SignerResult<()> {
    // INSERT OR IGNORE heals both fully-missing and partial (coin-without-p2) rows.
    let mut stmt = conn
        .prepare(
            r"
            SELECT offer_id, market_id, presplit_input_coin_id, maker_puzzle_hash
            FROM offer_state
            WHERE state IN ('open', 'refresh_due', 'mempool_observed', 'pending_visibility')
              AND (
                (presplit_input_coin_id IS NOT NULL AND length(trim(presplit_input_coin_id)) > 0)
                OR (maker_puzzle_hash IS NOT NULL AND length(trim(maker_puzzle_hash)) > 0)
              )
            ",
        )
        .map_err(|err| {
            SignerError::Other(format!(
                "failed to prepare offer_coin_watches backfill query: {err}"
            ))
        })?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|err| {
            SignerError::Other(format!(
                "failed to query offer_coin_watches backfill rows: {err}"
            ))
        })?;
    let now = chrono::Utc::now().to_rfc3339();
    for row in rows {
        let (offer_id, market_id, input_coin, maker_p2) = row.map_err(|err| {
            SignerError::Other(format!(
                "failed to read offer_coin_watches backfill row: {err}"
            ))
        })?;
        if let Some(coin_id) = input_coin
            .as_deref()
            .map(crate::hex::normalize_hex_id)
            .filter(|value| value.len() == 64)
        {
            conn.execute(
                r"
                INSERT OR IGNORE INTO offer_coin_watches (coin_id, offer_id, market_id, kind, updated_at)
                VALUES (?1, ?2, ?3, 'coin', ?4)
                ",
                params![coin_id, offer_id, market_id, now],
            )
            .map_err(|err| {
                SignerError::Other(format!(
                    "failed to backfill offer_coin_watches coin for {offer_id}: {err}"
                ))
            })?;
        }
        if let Some(p2_id) = maker_p2
            .as_deref()
            .map(crate::hex::normalize_hex_id)
            .filter(|value| value.len() == 64)
        {
            conn.execute(
                r"
                INSERT OR IGNORE INTO offer_coin_watches (coin_id, offer_id, market_id, kind, updated_at)
                VALUES (?1, ?2, ?3, 'p2', ?4)
                ",
                params![p2_id, offer_id, market_id, now],
            )
            .map_err(|err| {
                SignerError::Other(format!(
                    "failed to backfill offer_coin_watches p2 for {offer_id}: {err}"
                ))
            })?;
        }
    }
    Ok(())
}

fn ensure_offer_coin_watches_table(conn: &Connection) -> SignerResult<()> {
    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS offer_coin_watches (
          coin_id TEXT NOT NULL,
          offer_id TEXT NOT NULL,
          market_id TEXT NOT NULL,
          kind TEXT NOT NULL DEFAULT 'coin',
          updated_at TEXT NOT NULL,
          PRIMARY KEY (coin_id, offer_id)
        );
        CREATE INDEX IF NOT EXISTS idx_offer_coin_watches_market
          ON offer_coin_watches(market_id);
        CREATE INDEX IF NOT EXISTS idx_offer_coin_watches_offer
          ON offer_coin_watches(offer_id);
        ",
    )
    .map_err(|err| SignerError::Other(format!("offer_coin_watches migrate: {err}")))?;
    Ok(())
}

fn normalize_legacy_tx_id_storage(conn: &Connection) -> SignerResult<()> {
    normalize_tx_signal_state_ids(conn)?;
    normalize_offer_cancel_submitted_tx_ids(conn)?;
    Ok(())
}

fn backfill_offer_cancel_submitted_at(conn: &Connection) -> SignerResult<()> {
    conn.execute(
        r"
        UPDATE offer_state
        SET cancel_submitted_at = updated_at
        WHERE state = 'cancel_submitted'
          AND cancel_submitted_at IS NULL
        ",
        [],
    )
    .map_err(|err| {
        SignerError::Other(format!(
            "failed to backfill offer_state cancel_submitted_at: {err}"
        ))
    })?;
    Ok(())
}

fn normalize_tx_signal_state_ids(conn: &Connection) -> SignerResult<()> {
    let mut stmt = conn
        .prepare(
            r"
            SELECT tx_id, mempool_observed_at, tx_block_confirmed_at
            FROM tx_signal_state
            ",
        )
        .map_err(|err| {
            SignerError::Other(format!(
                "failed to prepare tx_signal_state migration query: {err}"
            ))
        })?;
    let mut rows = stmt.query([]).map_err(|err| {
        SignerError::Other(format!(
            "failed to query tx_signal_state for migration: {err}"
        ))
    })?;
    let mut legacy_rows = Vec::new();
    while let Some(row) = rows.next().map_err(|err| {
        SignerError::Other(format!(
            "failed to read tx_signal_state migration row: {err}"
        ))
    })? {
        let raw_id: String = row.get(0).map_err(|err| {
            SignerError::Other(format!("failed to read tx_signal_state tx_id: {err}"))
        })?;
        let mempool: String = row.get(1).map_err(|err| {
            SignerError::Other(format!(
                "failed to read tx_signal_state mempool timestamp: {err}"
            ))
        })?;
        let confirmed: Option<String> = row.get(2).ok();
        legacy_rows.push((raw_id, mempool, confirmed));
    }
    for (raw_id, mempool, confirmed) in legacy_rows {
        let Some(canonical) = canonical_tx_id(&raw_id) else {
            continue;
        };
        if raw_id == canonical {
            continue;
        }
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
            SignerError::Other(format!(
                "failed to migrate tx_signal_state id {raw_id} -> {canonical}: {err}"
            ))
        })?;
        conn.execute(
            "DELETE FROM tx_signal_state WHERE tx_id = ?1",
            params![raw_id],
        )
        .map_err(|err| {
            SignerError::Other(format!(
                "failed to delete legacy tx_signal_state id {raw_id}: {err}"
            ))
        })?;
    }
    Ok(())
}

fn normalize_offer_cancel_submitted_tx_ids(conn: &Connection) -> SignerResult<()> {
    let mut stmt = conn
        .prepare(
            r"
            SELECT offer_id, cancel_submitted_tx_id
            FROM offer_state
            WHERE cancel_submitted_tx_id IS NOT NULL
            ",
        )
        .map_err(|err| {
            SignerError::Other(format!(
                "failed to prepare offer_state cancel tx migration query: {err}"
            ))
        })?;
    let mut rows = stmt.query([]).map_err(|err| {
        SignerError::Other(format!(
            "failed to query offer_state cancel tx ids for migration: {err}"
        ))
    })?;
    let mut updates = Vec::new();
    while let Some(row) = rows.next().map_err(|err| {
        SignerError::Other(format!(
            "failed to read offer_state cancel tx migration row: {err}"
        ))
    })? {
        let offer_id: String = row.get(0).map_err(|err| {
            SignerError::Other(format!(
                "failed to read offer_id for cancel tx migration: {err}"
            ))
        })?;
        let raw_id: String = row.get(1).map_err(|err| {
            SignerError::Other(format!(
                "failed to read cancel_submitted_tx_id for migration: {err}"
            ))
        })?;
        updates.push((offer_id, raw_id));
    }
    for (offer_id, raw_id) in updates {
        let Some(canonical) = canonical_tx_id(&raw_id) else {
            continue;
        };
        if raw_id == canonical {
            continue;
        }
        conn.execute(
            "UPDATE offer_state SET cancel_submitted_tx_id = ?1 WHERE offer_id = ?2",
            params![canonical, offer_id],
        )
        .map_err(|err| {
            SignerError::Other(format!(
                "failed to migrate offer_state cancel_submitted_tx_id for {offer_id}: {err}"
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::offer::types::{OfferExecutionMode, PresplitCancelFields};
    use crate::storage::sqlite::{OfferCancelWrite, SqliteStore};

    use super::super::utcnow_iso;

    #[test]
    fn migration_heals_partial_coin_watch_with_missing_p2() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let offer_id = "ab".repeat(32);
        let coin = "cd".repeat(32);
        let meta_p2 = "ef".repeat(32);
        {
            let store = SqliteStore::open(&db_path).expect("open");
            let fields = PresplitCancelFields {
                input_coin_id: Some(coin.clone()),
                fixed_delegated_puzzle_hash: Some("aa".repeat(32)),
                maker_puzzle_hash: Some(meta_p2.clone()),
            };
            store
                .upsert_offer_state_with_metadata_at(
                    &offer_id,
                    "m1",
                    "open",
                    None,
                    &utcnow_iso(),
                    OfferCancelWrite {
                        fields: Some(&fields),
                        execution_mode: Some(OfferExecutionMode::PresplitExisting),
                        publish_venue: Some("coinset"),
                        ..OfferCancelWrite::default()
                    },
                )
                .expect("upsert metadata");
            // Coin-only watch (pre-upgrade partial row).
            store
                .replace_offer_coin_watches(&offer_id, "m1", std::slice::from_ref(&coin), &[])
                .expect("coin watch");
            assert!(store
                .list_offer_ids_for_watched_coin(&meta_p2)
                .expect("no p2 yet")
                .is_empty());
            // Clear one-shot flag so reopen re-runs watch/venue backfill.
            store
                .conn
                .execute(
                    "DELETE FROM schema_meta WHERE key IN ('watch_venue_backfill_v1', 'watch_venue_backfill_v2')",
                    [],
                )
                .expect("clear schema_meta");
        }
        // Re-open re-runs one-shot backfill; INSERT OR IGNORE heals missing p2.
        let store = SqliteStore::open(&db_path).expect("reopen");
        assert_eq!(
            store
                .list_offer_ids_for_watched_coin(&meta_p2)
                .expect("healed p2"),
            vec![offer_id.clone()]
        );
        assert_eq!(
            store.list_offer_ids_for_watched_coin(&coin).expect("coin"),
            vec![offer_id]
        );
    }

    #[test]
    fn migration_seeds_watches_from_cancel_metadata_when_absent() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let offer_id = "ab".repeat(32);
        let coin = "cd".repeat(32);
        let meta_p2 = "ef".repeat(32);
        {
            let store = SqliteStore::open(&db_path).expect("open");
            let fields = PresplitCancelFields {
                input_coin_id: Some(coin.clone()),
                fixed_delegated_puzzle_hash: Some("aa".repeat(32)),
                maker_puzzle_hash: Some(meta_p2.clone()),
            };
            store
                .upsert_offer_state_with_metadata_at(
                    &offer_id,
                    "m1",
                    "open",
                    None,
                    &utcnow_iso(),
                    OfferCancelWrite {
                        fields: Some(&fields),
                        execution_mode: Some(OfferExecutionMode::PresplitExisting),
                        publish_venue: Some("coinset"),
                        ..OfferCancelWrite::default()
                    },
                )
                .expect("upsert metadata");
            assert!(store
                .list_watched_coin_ids_for_market("m1")
                .expect("empty")
                .is_empty());
            store
                .conn
                .execute(
                    "DELETE FROM schema_meta WHERE key IN ('watch_venue_backfill_v1', 'watch_venue_backfill_v2')",
                    [],
                )
                .expect("clear schema_meta");
        }
        let store = SqliteStore::open(&db_path).expect("reopen");
        let watched = store.list_watched_coin_ids_for_market("m1").expect("coins");
        assert!(watched.contains(&coin));
        assert!(!watched.contains(&meta_p2));
        assert_eq!(
            store
                .list_offer_ids_for_watched_coin(&meta_p2)
                .expect("meta p2"),
            vec![offer_id]
        );
    }
}
