use rusqlite::Connection;

use crate::error::{SignerError, SignerResult};

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
    Ok(())
}
