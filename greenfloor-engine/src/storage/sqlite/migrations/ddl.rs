//! One-shot DDL rebuilds (table shape changes).

use rusqlite::Connection;

use super::super::db_err;
use super::helpers::column_exists;
use crate::error::SignerResult;
use crate::storage::schema::{
    offer_state_column_names_sql, offer_state_columns_ddl, OFFER_COIN_WATCHES_COLUMNS_DDL,
    OFFER_COIN_WATCHES_INDEXES_SQL,
};

const COPY_LEGACY_CANCEL_INPUT_SQL: &str = r"
UPDATE offer_state
SET cancel_input_coin_id = presplit_input_coin_id
WHERE (cancel_input_coin_id IS NULL OR length(trim(cancel_input_coin_id)) = 0)
  AND presplit_input_coin_id IS NOT NULL
  AND length(trim(presplit_input_coin_id)) > 0
";

/// Rebuild so PRIMARY KEY includes `kind` (coin and p2 may share the same 64-hex).
pub(super) fn rebuild_offer_coin_watches_pk(conn: &Connection) -> SignerResult<()> {
    let sql = format!(
        r"
        CREATE TABLE IF NOT EXISTS offer_coin_watches_new (
        {OFFER_COIN_WATCHES_COLUMNS_DDL}
        );
        INSERT OR IGNORE INTO offer_coin_watches_new
          (coin_id, offer_id, market_id, kind, updated_at)
        SELECT coin_id, offer_id, market_id, kind, updated_at FROM offer_coin_watches;
        DROP TABLE offer_coin_watches;
        ALTER TABLE offer_coin_watches_new RENAME TO offer_coin_watches;
        {OFFER_COIN_WATCHES_INDEXES_SQL}
        "
    );
    conn.execute_batch(&sql)
        .map_err(|err| db_err("offer_coin_watches pk(kind) migrate", err))?;
    Ok(())
}

/// Copy legacy `presplit_input_coin_id` into `cancel_input_coin_id`, then drop it.
///
/// Older DBs may still have `cancel_input_coin_id_rename_v1` marked; that key is inert.
pub(super) fn drop_presplit_input_coin_id(conn: &Connection) -> SignerResult<()> {
    if !column_exists(conn, "offer_state", "presplit_input_coin_id")? {
        return Ok(());
    }
    conn.execute(COPY_LEGACY_CANCEL_INPUT_SQL, [])
        .map_err(|err| db_err("cancel_input_coin_id pre-drop backfill", err))?;
    let columns = offer_state_columns_ddl();
    let names = offer_state_column_names_sql();
    let sql = format!(
        r"
        CREATE TABLE offer_state_new (
        {columns}
        );
        INSERT INTO offer_state_new ({names})
        SELECT {names}
        FROM offer_state;
        DROP TABLE offer_state;
        ALTER TABLE offer_state_new RENAME TO offer_state;
        "
    );
    conn.execute_batch(&sql)
        .map_err(|err| db_err("offer_state drop presplit_input_coin_id", err))?;
    Ok(())
}
