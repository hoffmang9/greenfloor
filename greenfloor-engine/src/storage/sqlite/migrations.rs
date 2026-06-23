use rusqlite::{params, Connection};

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
    add_column_if_missing(conn, "offer_state", "execution_mode", "TEXT NULL")?;
    add_column_if_missing(conn, "offer_state", "cancel_submitted_tx_id", "TEXT NULL")?;
    add_column_if_missing(conn, "offer_state", "cancel_submitted_at", "TEXT NULL")?;
    backfill_offer_cancel_submitted_at(conn)?;
    normalize_legacy_tx_id_storage(conn)?;
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
    use rusqlite::{params, Connection};

    use crate::hex::legacy_prefixed_tx_id;
    use crate::storage::schema::SCHEMA;
    use crate::storage::SqliteStore;

    #[test]
    fn normalize_legacy_tx_signal_ids_on_store_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("migrate.db");
        let canonical = "a".repeat(64);
        let legacy = legacy_prefixed_tx_id(&canonical).expect("legacy id");
        {
            let conn = Connection::open(&path).expect("open");
            conn.execute_batch(SCHEMA).expect("schema");
            conn.execute(
                "INSERT INTO tx_signal_state (tx_id, mempool_observed_at) VALUES (?1, ?2)",
                params![legacy, "2020-01-01T00:00:00Z"],
            )
            .expect("insert legacy tx id");
        }
        let store = SqliteStore::open(&path).expect("open store");
        let state = store
            .get_tx_signal_state(std::slice::from_ref(&canonical))
            .expect("lookup canonical");
        assert!(state.contains_key(&canonical));
        let conn = Connection::open(&path).expect("reopen");
        let legacy_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tx_signal_state WHERE tx_id = ?1",
                params![legacy],
                |row| row.get(0),
            )
            .expect("count legacy");
        assert_eq!(legacy_count, 0);
    }

    #[test]
    fn normalize_legacy_offer_cancel_tx_id_on_store_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("migrate.db");
        let canonical = "b".repeat(64);
        let legacy = legacy_prefixed_tx_id(&canonical).expect("legacy id");
        {
            let conn = Connection::open(&path).expect("open");
            conn.execute_batch(SCHEMA).expect("schema");
            conn.execute(
                r"
                INSERT INTO offer_state
                  (offer_id, market_id, state, updated_at, cancel_submitted_tx_id)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ",
                params![
                    "offer-1",
                    "m1",
                    "cancel_submitted",
                    "2020-01-01T00:00:00Z",
                    legacy
                ],
            )
            .expect("insert legacy cancel tx id");
        }
        let store = SqliteStore::open(&path).expect("open store");
        let row = store
            .list_offer_states_for_ids(&["offer-1".to_string()])
            .expect("rows")
            .into_iter()
            .next()
            .expect("row");
        assert_eq!(
            row.cancel_submitted_tx_id.as_deref(),
            Some(canonical.as_str())
        );
    }

    #[test]
    fn backfill_cancel_submitted_at_from_updated_at_on_store_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("migrate.db");
        {
            let conn = Connection::open(&path).expect("open");
            conn.execute_batch(SCHEMA).expect("schema");
            conn.execute(
                r"
                INSERT INTO offer_state
                  (offer_id, market_id, state, updated_at, cancel_submitted_at)
                VALUES (?1, ?2, ?3, ?4, NULL)
                ",
                params!["offer-1", "m1", "cancel_submitted", "2020-01-01T00:00:00Z"],
            )
            .expect("insert cancel_submitted without timestamp");
        }
        let store = SqliteStore::open(&path).expect("open store");
        let row = store
            .list_offer_states_for_ids(&["offer-1".to_string()])
            .expect("rows")
            .into_iter()
            .next()
            .expect("row");
        assert_eq!(
            row.cancel_submitted_at.as_deref(),
            Some("2020-01-01T00:00:00Z")
        );
    }
}
