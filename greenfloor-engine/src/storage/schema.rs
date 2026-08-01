//! Canonical `SQLite` bootstrap DDL.
//!
//! `offer_state` extras and `offer_coin_watches` column/index fragments are the single
//! source of truth for bootstrap schema and rebuild migrations.

use std::sync::OnceLock;

/// Core `offer_state` columns present since the original bootstrap shape.
const OFFER_STATE_CORE_COLUMNS_DDL: &str = r"
  offer_id TEXT PRIMARY KEY,
  market_id TEXT NOT NULL,
  state TEXT NOT NULL,
  last_seen_status INTEGER NULL,
  updated_at TEXT NOT NULL
";

const OFFER_STATE_CORE_COLUMN_NAMES: &[&str] = &[
    "offer_id",
    "market_id",
    "state",
    "last_seen_status",
    "updated_at",
];

/// Nullable columns added after the original `offer_state` bootstrap shape.
///
/// Used for `ALTER TABLE … ADD COLUMN` upgrades and folded into bootstrap/`rebuild` DDL.
pub(crate) const OFFER_STATE_ADDITIVE_COLUMNS: &[(&str, &str)] = &[
    ("cancel_input_coin_id", "TEXT NULL"),
    ("fixed_delegated_puzzle_hash", "TEXT NULL"),
    ("maker_puzzle_hash", "TEXT NULL"),
    ("execution_mode", "TEXT NULL"),
    ("cancel_submitted_tx_id", "TEXT NULL"),
    ("cancel_submitted_at", "TEXT NULL"),
    ("publish_venue", "TEXT NULL"),
    // Soft listing expiry (unix seconds); stable makers omit on-chain CONDITIONS expiry.
    ("listing_expires_at", "INTEGER NULL"),
    ("size_base_units", "INTEGER NULL"),
    ("offer_nonce", "TEXT NULL"),
    ("offer_side", "TEXT NULL"),
];

pub(crate) const OFFER_COIN_WATCHES_COLUMNS_DDL: &str = r"
  coin_id TEXT NOT NULL,
  offer_id TEXT NOT NULL,
  market_id TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'coin',
  updated_at TEXT NOT NULL,
  PRIMARY KEY (coin_id, offer_id, kind)
";

pub(crate) const OFFER_COIN_WATCHES_INDEXES_SQL: &str = r"
CREATE INDEX IF NOT EXISTS idx_offer_coin_watches_market
  ON offer_coin_watches(market_id);
CREATE INDEX IF NOT EXISTS idx_offer_coin_watches_offer
  ON offer_coin_watches(offer_id);
";

/// Full `offer_state` column DDL (core + additive).
pub(crate) fn offer_state_columns_ddl() -> String {
    let mut ddl = OFFER_STATE_CORE_COLUMNS_DDL.trim_end().to_string();
    for (name, definition) in OFFER_STATE_ADDITIVE_COLUMNS {
        ddl.push_str(",\n  ");
        ddl.push_str(name);
        ddl.push(' ');
        ddl.push_str(definition);
    }
    ddl.push('\n');
    ddl
}

/// Comma-separated `offer_state` column names for `INSERT … SELECT` rebuilds.
pub(crate) fn offer_state_column_names_sql() -> String {
    let mut names = OFFER_STATE_CORE_COLUMN_NAMES.to_vec();
    names.extend(OFFER_STATE_ADDITIVE_COLUMNS.iter().map(|(name, _)| *name));
    names.join(", ")
}

fn build_schema() -> String {
    format!(
        r"
CREATE TABLE IF NOT EXISTS alert_state (
  market_id TEXT PRIMARY KEY,
  is_low INTEGER NOT NULL,
  last_alert_at TEXT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_event (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  event_type TEXT NOT NULL,
  market_id TEXT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS price_policy_history (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  market_id TEXT NOT NULL,
  source TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tx_signal_state (
  tx_id TEXT PRIMARY KEY,
  mempool_observed_at TEXT NOT NULL,
  tx_block_confirmed_at TEXT NULL
);

CREATE TABLE IF NOT EXISTS offer_state (
{offer_state_columns}
);

CREATE TABLE IF NOT EXISTS coin_op_ledger (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  market_id TEXT NOT NULL,
  op_type TEXT NOT NULL,
  op_count INTEGER NOT NULL,
  fee_mojos INTEGER NOT NULL,
  status TEXT NOT NULL,
  reason TEXT NOT NULL,
  operation_id TEXT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS offer_reservation_lease (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  reservation_id TEXT NOT NULL,
  market_id TEXT NOT NULL,
  wallet_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  amount INTEGER NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  released_at TEXT NULL
);

CREATE TABLE IF NOT EXISTS schema_meta (
  key TEXT PRIMARY KEY,
  applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS offer_coin_watches (
{watches_columns}
);
{watches_indexes}
",
        offer_state_columns = offer_state_columns_ddl(),
        watches_columns = OFFER_COIN_WATCHES_COLUMNS_DDL,
        watches_indexes = OFFER_COIN_WATCHES_INDEXES_SQL,
    )
}

/// Canonical bootstrap schema SQL (built once from shared DDL fragments).
pub fn schema_sql() -> &'static str {
    static SCHEMA: OnceLock<String> = OnceLock::new();
    SCHEMA.get_or_init(build_schema).as_str()
}
