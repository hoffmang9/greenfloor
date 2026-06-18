use rusqlite::params;
use serde_json::Value;

use crate::error::{SignerError, SignerResult};

use super::{utcnow_iso, AuditEventRow, SqliteStore};

impl SqliteStore {
    pub fn get_latest_xch_price_snapshot(&self) -> SignerResult<Option<f64>> {
        let mut stmt = self
            .conn
            .prepare(
                r"
            SELECT payload_json
            FROM audit_event
            WHERE event_type = 'xch_price_snapshot'
            ORDER BY id DESC
            LIMIT 1
            ",
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to prepare xch price snapshot query: {err}"))
            })?;
        let mut rows = stmt.query([]).map_err(|err| {
            SignerError::Other(format!("failed to query xch price snapshot: {err}"))
        })?;
        let Some(row) = rows
            .next()
            .map_err(|err| SignerError::Other(format!("failed to read xch price row: {err}")))?
        else {
            return Ok(None);
        };
        let payload_json: String = row
            .get(0)
            .map_err(|err| SignerError::Other(format!("failed to read payload_json: {err}")))?;
        let payload: Value = serde_json::from_str(&payload_json).map_err(|err| {
            SignerError::Other(format!("failed to decode xch price snapshot json: {err}"))
        })?;
        let Some(raw) = payload.get("price_usd") else {
            return Ok(None);
        };
        let value = raw
            .as_f64()
            .or_else(|| raw.as_i64().map(crate::offer::pricing::i64_to_f64))
            .ok_or_else(|| {
                SignerError::Other("xch_price_snapshot price_usd is not numeric".to_string())
            })?;
        if value <= 0.0 {
            return Ok(None);
        }
        Ok(Some(value))
    }

    pub fn list_recent_audit_events(
        &self,
        event_types: Option<&[&str]>,
        market_id: Option<&str>,
        limit: usize,
    ) -> SignerResult<Vec<AuditEventRow>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit_i64 = i64::try_from(limit).map_err(|_| {
            SignerError::Other("list_recent_audit_events limit exceeds i64 max".to_string())
        })?;
        let mut where_clauses = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(types) = event_types.filter(|values| !values.is_empty()) {
            let placeholders = types.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("event_type IN ({placeholders})"));
            for event_type in types {
                params.push(Box::new(event_type.to_string()));
            }
        }
        if let Some(market_id) = market_id.map(str::trim).filter(|value| !value.is_empty()) {
            where_clauses.push("market_id = ?".to_string());
            params.push(Box::new(market_id.to_string()));
        }
        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };
        let sql = format!(
            r"
            SELECT id, event_type, market_id, payload_json, created_at
            FROM audit_event
            {where_sql}
            ORDER BY id DESC
            LIMIT ?
            "
        );
        params.push(Box::new(limit_i64));
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(std::convert::AsRef::as_ref).collect();
        let mut stmt = self.conn.prepare(&sql).map_err(|err| {
            SignerError::Other(format!("failed to prepare audit_event query: {err}"))
        })?;
        let mut rows = stmt
            .query(param_refs.as_slice())
            .map_err(|err| SignerError::Other(format!("failed to query audit_event: {err}")))?;
        let mut out = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|err| SignerError::Other(format!("failed to read audit_event row: {err}")))?
        {
            let payload_json: String = row
                .get(3)
                .map_err(|err| SignerError::Other(format!("failed to read payload_json: {err}")))?;
            let payload: Value =
                serde_json::from_str(&payload_json).unwrap_or(Value::String(payload_json));
            out.push(AuditEventRow {
                id: row
                    .get(0)
                    .map_err(|err| SignerError::Other(format!("failed to read audit id: {err}")))?,
                event_type: row.get(1).map_err(|err| {
                    SignerError::Other(format!("failed to read audit event_type: {err}"))
                })?,
                market_id: row.get(2).ok(),
                payload,
                created_at: row.get(4).map_err(|err| {
                    SignerError::Other(format!("failed to read audit created_at: {err}"))
                })?,
            });
        }
        Ok(out)
    }

    pub fn add_audit_event(
        &self,
        event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) -> SignerResult<()> {
        self.add_audit_event_at(event_type, payload, market_id, &utcnow_iso())
    }

    pub fn add_audit_event_at(
        &self,
        event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
        created_at: &str,
    ) -> SignerResult<()> {
        let payload_json = serde_json::to_string(payload).map_err(|err| {
            SignerError::Other(format!("failed to encode audit payload json: {err}"))
        })?;
        self.conn
            .execute(
                r"
                INSERT INTO audit_event (event_type, market_id, payload_json, created_at)
                VALUES (?1, ?2, ?3, ?4)
                ",
                params![event_type, market_id, payload_json, created_at],
            )
            .map_err(|err| SignerError::Other(format!("failed to insert audit_event: {err}")))?;
        Ok(())
    }
}
