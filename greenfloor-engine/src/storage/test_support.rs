//! Hidden helpers for integration tests that seed legacy DB state before migrations run.
#![cfg_attr(coverage, coverage(off))]

use std::path::Path;

use rusqlite::Connection;

use crate::error::SignerResult;

use super::schema::SCHEMA;

/// Open a database file and apply the canonical schema without running store migrations.
#[doc(hidden)]
pub fn open_pre_migration_connection(path: &Path) -> SignerResult<Connection> {
    let conn = Connection::open(path).map_err(|err| {
        crate::error::SignerError::Other(format!("open pre-migration sqlite db: {err}"))
    })?;
    conn.execute_batch(SCHEMA).map_err(|err| {
        crate::error::SignerError::Other(format!("apply pre-migration schema: {err}"))
    })?;
    Ok(conn)
}
