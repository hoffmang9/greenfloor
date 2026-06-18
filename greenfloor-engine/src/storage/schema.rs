pub const SCHEMA: &str = r"
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
  offer_id TEXT PRIMARY KEY,
  market_id TEXT NOT NULL,
  state TEXT NOT NULL,
  last_seen_status INTEGER NULL,
  updated_at TEXT NOT NULL
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
";
