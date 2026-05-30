use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

use crate::cycle::OfferLifecycleState;
use crate::error::{SignerError, SignerResult};

const SCHEMA: &str = r#"
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
"#;

#[derive(Debug, Clone)]
pub struct OfferPostPersistRecord {
    pub offer_id: String,
    pub market_id: String,
    pub side: String,
    pub size_base_units: u64,
    pub publish_venue: String,
    pub resolved_base_asset_id: String,
    pub resolved_quote_asset_id: String,
    pub created_extra: Value,
}

pub struct SqliteStore {
    conn: Connection,
}

pub fn state_db_path_for_home(home_dir: &Path) -> PathBuf {
    home_dir.join("db").join("greenfloor.sqlite")
}

impl SqliteStore {
    pub fn open(db_path: &Path) -> SignerResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                SignerError::Other(format!(
                    "failed to create sqlite parent dir {}: {err}",
                    parent.display()
                ))
            })?;
        }
        let conn = Connection::open(db_path).map_err(|err| {
            SignerError::Other(format!(
                "failed to open sqlite db {}: {err}",
                db_path.display()
            ))
        })?;
        conn.busy_timeout(Duration::from_secs(30)).map_err(|err| {
            SignerError::Other(format!("failed to set sqlite busy_timeout: {err}"))
        })?;
        conn.execute_batch("PRAGMA busy_timeout = 30000;")
            .map_err(|err| SignerError::Other(format!("failed to set busy_timeout pragma: {err}")))?;
        conn.execute_batch(SCHEMA).map_err(|err| {
            SignerError::Other(format!("failed to initialize sqlite schema: {err}"))
        })?;
        Ok(Self { conn })
    }

    pub fn upsert_offer_state(
        &self,
        offer_id: &str,
        market_id: &str,
        state: &str,
        last_seen_status: Option<i64>,
    ) -> SignerResult<()> {
        self.conn
            .execute(
                r#"
                INSERT INTO offer_state (offer_id, market_id, state, last_seen_status, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(offer_id) DO UPDATE SET
                  market_id = excluded.market_id,
                  state = excluded.state,
                  last_seen_status = excluded.last_seen_status,
                  updated_at = excluded.updated_at
                "#,
                params![
                    offer_id,
                    market_id,
                    state,
                    last_seen_status,
                    utcnow_iso(),
                ],
            )
            .map_err(|err| SignerError::Other(format!("failed to upsert offer_state: {err}")))?;
        Ok(())
    }

    pub fn add_audit_event(
        &self,
        event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) -> SignerResult<()> {
        let payload_json = serde_json::to_string(payload).map_err(|err| {
            SignerError::Other(format!("failed to encode audit payload json: {err}"))
        })?;
        self.conn
            .execute(
                r#"
                INSERT INTO audit_event (event_type, market_id, payload_json, created_at)
                VALUES (?1, ?2, ?3, ?4)
                "#,
                params![event_type, market_id, payload_json, utcnow_iso()],
            )
            .map_err(|err| SignerError::Other(format!("failed to insert audit_event: {err}")))?;
        Ok(())
    }

    pub(crate) fn offer_state_for_id(&self, offer_id: &str) -> SignerResult<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT state FROM offer_state WHERE offer_id = ?1")
            .map_err(|err| SignerError::Other(format!("failed to prepare offer_state query: {err}")))?;
        let mut rows = stmt
            .query(params![offer_id])
            .map_err(|err| SignerError::Other(format!("failed to query offer_state: {err}")))?;
        if let Some(row) = rows
            .next()
            .map_err(|err| SignerError::Other(format!("failed to read offer_state row: {err}")))?
        {
            let state: String = row
                .get(0)
                .map_err(|err| SignerError::Other(format!("failed to read offer state: {err}")))?;
            return Ok(Some(state));
        }
        Ok(None)
    }

    #[cfg(test)]
    fn count_audit_events(&self, event_type: &str, market_id: &str) -> SignerResult<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM audit_event WHERE event_type = ?1 AND market_id = ?2",
                params![event_type, market_id],
                |row| row.get(0),
            )
            .map_err(|err| SignerError::Other(format!("failed to count audit events: {err}")))
    }

    #[cfg(test)]
    fn latest_audit_payload(
        &self,
        event_type: &str,
        market_id: &str,
    ) -> SignerResult<Option<Value>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT payload_json
                FROM audit_event
                WHERE event_type = ?1 AND market_id = ?2
                ORDER BY id DESC
                LIMIT 1
                "#,
            )
            .map_err(|err| SignerError::Other(format!("failed to prepare audit query: {err}")))?;
        let mut rows = stmt
            .query(params![event_type, market_id])
            .map_err(|err| SignerError::Other(format!("failed to query audit events: {err}")))?;
        if let Some(row) = rows
            .next()
            .map_err(|err| SignerError::Other(format!("failed to read audit row: {err}")))?
        {
            let payload_json: String = row
                .get(0)
                .map_err(|err| SignerError::Other(format!("failed to read payload_json: {err}")))?;
            let payload: Value = serde_json::from_str(&payload_json).map_err(|err| {
                SignerError::Other(format!("failed to decode audit payload json: {err}"))
            })?;
            return Ok(Some(payload));
        }
        Ok(None)
    }
}

pub fn persist_offer_post_records(
    store: &SqliteStore,
    records: &[OfferPostPersistRecord],
) -> SignerResult<()> {
    for record in records {
        store.upsert_offer_state(
            &record.offer_id,
            &record.market_id,
            OfferLifecycleState::Open.as_str(),
            None,
        )?;
        let mut audit_event = json!({
            "market_id": record.market_id,
            "planned_count": 1,
            "executed_count": 1,
            "items": [{
                "size": record.size_base_units,
                "side": record.side,
                "status": "executed",
                "reason": format!("{}_post_success", record.publish_venue),
                "offer_id": record.offer_id,
                "attempts": 1,
            }],
            "venue": record.publish_venue,
            "resolved_base_asset_id": record.resolved_base_asset_id,
            "resolved_quote_asset_id": record.resolved_quote_asset_id,
        });
        if let Value::Object(extra) = &record.created_extra {
            if let Value::Object(audit_obj) = &mut audit_event {
                for (key, value) in extra {
                    audit_obj.insert(key.clone(), value.clone());
                }
            }
        }
        store.add_audit_event(
            "strategy_offer_execution",
            &audit_event,
            Some(record.market_id.as_str()),
        )?;
    }
    Ok(())
}

fn utcnow_iso() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persist_offer_post_records_writes_offer_state_and_audit_event() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        let store = SqliteStore::open(&db_path).expect("open");

        persist_offer_post_records(
            &store,
            &[OfferPostPersistRecord {
                offer_id: "offer-123".to_string(),
                market_id: "m1".to_string(),
                side: "sell".to_string(),
                size_base_units: 10,
                publish_venue: "dexie".to_string(),
                resolved_base_asset_id: "a1".to_string(),
                resolved_quote_asset_id: "xch".to_string(),
                created_extra: json!({"execution_mode": "direct"}),
            }],
        )
        .expect("persist");

        assert_eq!(
            store
                .offer_state_for_id("offer-123")
                .expect("offer state")
                .as_deref(),
            Some("open")
        );

        let count = store
            .count_audit_events("strategy_offer_execution", "m1")
            .expect("count");
        assert_eq!(count, 1);

        let payload = store
            .latest_audit_payload("strategy_offer_execution", "m1")
            .expect("payload")
            .expect("row");
        let items = payload
            .get("items")
            .and_then(Value::as_array)
            .expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].get("offer_id").and_then(Value::as_str),
            Some("offer-123")
        );
        assert_eq!(
            payload.get("execution_mode").and_then(Value::as_str),
            Some("direct")
        );
    }
}
