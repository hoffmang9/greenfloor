//! Additive schema migrations after base `CREATE TABLE IF NOT EXISTS` bootstrap.

mod data;
mod ddl;
mod helpers;

#[cfg(test)]
mod tests;

use rusqlite::Connection;

use data::{
    backfill_offer_cancel_submitted_at, backfill_watches_and_venue, normalize_legacy_tx_id_storage,
};
use ddl::{drop_presplit_input_coin_id, rebuild_offer_coin_watches_pk};
use helpers::{add_column_if_missing, run_once};

use crate::error::SignerResult;
use crate::storage::schema::OFFER_STATE_ADDITIVE_COLUMNS;

enum Step {
    /// `ALTER TABLE offer_state ADD COLUMN …` for pre-bootstrap shapes.
    Columns(&'static [(&'static str, &'static str)]),
    /// Run once; gated by `schema_meta` key.
    Once(&'static str, fn(&Connection) -> SignerResult<()>),
    /// Idempotent data fixups safe to re-run every open.
    Always(fn(&Connection) -> SignerResult<()>),
}

const STEPS: &[Step] = &[
    Step::Columns(OFFER_STATE_ADDITIVE_COLUMNS),
    Step::Once(
        "offer_coin_watches_pk_kind_v1",
        rebuild_offer_coin_watches_pk,
    ),
    Step::Once(
        "cancel_input_coin_id_drop_presplit_v1",
        drop_presplit_input_coin_id,
    ),
    Step::Always(backfill_offer_cancel_submitted_at),
    Step::Always(normalize_legacy_tx_id_storage),
    Step::Once("watch_venue_backfill_v2", backfill_watches_and_venue),
];

/// Apply additive schema migrations after base `CREATE TABLE IF NOT EXISTS` bootstrap.
///
/// # Errors
///
/// Returns an error if a migration fails for reasons other than idempotent re-run.
pub(crate) fn apply_schema_migrations(conn: &Connection) -> SignerResult<()> {
    // Do not re-add legacy `presplit_input_coin_id` — the drop-presplit step copies
    // then rebuilds without it when the old column is still present on upgraded DBs.
    for step in STEPS {
        match *step {
            Step::Columns(columns) => {
                for (column, definition) in columns {
                    add_column_if_missing(conn, "offer_state", column, definition)?;
                }
            }
            Step::Once(key, run) => run_once(conn, key, run)?,
            Step::Always(run) => run(conn)?,
        }
    }
    Ok(())
}
