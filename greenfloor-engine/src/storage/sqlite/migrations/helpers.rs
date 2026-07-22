use rusqlite::{params, Connection, OptionalExtension, Row};

use super::super::db_err;
use crate::error::SignerResult;

pub(super) fn column_exists(conn: &Connection, table: &str, column: &str) -> SignerResult<bool> {
    let names = query_mapped(
        conn,
        &format!("PRAGMA table_info({table})"),
        &format!("pragma table_info {table}"),
        |row| row.get::<_, String>(1),
    )?;
    Ok(names.iter().any(|name| name == column))
}

pub(super) fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> SignerResult<()> {
    if column_exists(conn, table, column)? {
        return Ok(());
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )
    .map_err(|err| db_err(&format!("add column {table}.{column}"), err))?;
    Ok(())
}

pub(super) fn schema_meta_applied(conn: &Connection, key: &str) -> SignerResult<bool> {
    let found: Option<String> = conn
        .query_row(
            "SELECT key FROM schema_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| db_err(&format!("read schema_meta key {key}"), err))?;
    Ok(found.is_some())
}

pub(super) fn mark_schema_meta_applied(conn: &Connection, key: &str) -> SignerResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO schema_meta (key, applied_at) VALUES (?1, ?2)",
        params![key, chrono::Utc::now().to_rfc3339()],
    )
    .map_err(|err| db_err(&format!("mark schema_meta key {key}"), err))?;
    Ok(())
}

/// Run `step` once, gated by `schema_meta` key.
pub(super) fn run_once(
    conn: &Connection,
    key: &str,
    step: fn(&Connection) -> SignerResult<()>,
) -> SignerResult<()> {
    if schema_meta_applied(conn, key)? {
        return Ok(());
    }
    step(conn)?;
    mark_schema_meta_applied(conn, key)
}

/// Prepare + `query_map` + collect with consistent error context.
pub(super) fn query_mapped<T, F>(
    conn: &Connection,
    sql: &str,
    context: &str,
    map_row: F,
) -> SignerResult<Vec<T>>
where
    F: FnMut(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut stmt = conn
        .prepare(sql)
        .map_err(|err| db_err(&format!("prepare {context}"), err))?;
    let rows = stmt
        .query_map([], map_row)
        .map_err(|err| db_err(&format!("query {context}"), err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_err(&format!("read {context}"), err))
}

/// Canonical tx/coin id when it differs from the stored form.
pub(super) fn rewritten_tx_id(raw: &str) -> Option<String> {
    let canonical = crate::hex::canonical_tx_id(raw)?;
    (canonical != raw).then_some(canonical)
}
