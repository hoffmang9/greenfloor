use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::error::{SignerError, SignerResult};

use super::SqliteStore;

impl SqliteStore {
    pub fn try_acquire_offer_reservation_lease(
        &self,
        reservation_id: &str,
        market_id: &str,
        wallet_id: &str,
        requested_amounts: &BTreeMap<String, i64>,
        available_amounts: &BTreeMap<String, i64>,
        lease_seconds: i64,
        now: Option<DateTime<Utc>>,
    ) -> SignerResult<Option<String>> {
        if reservation_id.trim().is_empty() {
            return Err(SignerError::Other("reservation_id is required".to_string()));
        }
        if lease_seconds <= 0 {
            return Err(SignerError::Other("lease_seconds must be > 0".to_string()));
        }
        let normalized_requests: BTreeMap<String, i64> = requested_amounts
            .iter()
            .filter_map(|(asset_id, amount)| {
                let amount = *amount;
                (amount > 0).then_some((asset_id.trim().to_ascii_lowercase(), amount))
            })
            .collect();
        if normalized_requests.is_empty() {
            return Ok(Some("reservation_empty_request".to_string()));
        }
        let normalized_available: BTreeMap<String, i64> = available_amounts
            .iter()
            .filter_map(|(asset_id, amount)| {
                let amount = *amount;
                (amount > 0).then_some((asset_id.trim().to_ascii_lowercase(), amount))
            })
            .collect();
        let now_dt = now.unwrap_or_else(Utc::now);
        let now_iso = now_dt.to_rfc3339();
        let expires_at_iso = (now_dt + chrono::Duration::seconds(lease_seconds)).to_rfc3339();

        self.conn.execute("BEGIN IMMEDIATE", []).map_err(|err| {
            SignerError::Other(format!("failed to begin reservation transaction: {err}"))
        })?;
        let result = (|| -> SignerResult<Option<String>> {
            self.conn.execute(
                r#"
                UPDATE offer_reservation_lease
                SET status = 'expired',
                    released_at = COALESCE(released_at, ?1)
                WHERE status = 'active'
                  AND expires_at <= ?2
                "#,
                params![now_iso, now_iso],
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to expire stale reservation leases: {err}"))
            })?;
            let mut stmt = self.conn.prepare(
                r#"
                SELECT asset_id, COALESCE(SUM(amount), 0) AS reserved_amount
                FROM offer_reservation_lease
                WHERE wallet_id = ?1
                  AND status = 'active'
                  AND expires_at > ?2
                GROUP BY asset_id
                "#,
            ).map_err(|err| {
                SignerError::Other(format!("failed to prepare reserved amounts query: {err}"))
            })?;
            let mut rows = stmt.query(params![wallet_id, now_iso]).map_err(|err| {
                SignerError::Other(format!("failed to query reserved amounts: {err}"))
            })?;
            let mut reserved_by_asset: BTreeMap<String, i64> = BTreeMap::new();
            while let Some(row) = rows.next().map_err(|err| {
                SignerError::Other(format!("failed to read reserved amount row: {err}"))
            })? {
                let asset_id: String = row.get(0).map_err(|err| {
                    SignerError::Other(format!("failed to read asset_id: {err}"))
                })?;
                let reserved: i64 = row.get(1).map_err(|err| {
                    SignerError::Other(format!("failed to read reserved_amount: {err}"))
                })?;
                reserved_by_asset.insert(asset_id.trim().to_ascii_lowercase(), reserved);
            }
            for (asset_id, amount) in &normalized_requests {
                let available = normalized_available.get(asset_id).copied().unwrap_or(0);
                let already_reserved = reserved_by_asset.get(asset_id).copied().unwrap_or(0);
                if available - already_reserved < *amount {
                    return Ok(Some(format!(
                        "reservation_insufficient_{asset_id}:available={available}:reserved={already_reserved}:needed={amount}"
                    )));
                }
            }
            for (asset_id, amount) in &normalized_requests {
                self.conn.execute(
                    r#"
                    INSERT INTO offer_reservation_lease
                      (reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, NULL)
                    "#,
                    params![
                        reservation_id,
                        market_id,
                        wallet_id,
                        asset_id,
                        amount,
                        now_iso,
                        expires_at_iso,
                    ],
                )
                .map_err(|err| {
                    SignerError::Other(format!("failed to insert reservation lease: {err}"))
                })?;
            }
            Ok(None)
        })();
        match result {
            Ok(value) => {
                self.conn.execute("COMMIT", []).map_err(|err| {
                    SignerError::Other(format!("failed to commit reservation transaction: {err}"))
                })?;
                Ok(value)
            }
            Err(err) => {
                let _ = self.conn.execute("ROLLBACK", []);
                Err(err)
            }
        }
    }

    pub fn release_offer_reservation_lease(
        &self,
        reservation_id: &str,
        release_status: &str,
    ) -> SignerResult<u64> {
        let released_at = Utc::now().to_rfc3339();
        let changed = self
            .conn
            .execute(
                r#"
                UPDATE offer_reservation_lease
                SET status = ?1, released_at = ?2
                WHERE reservation_id = ?3
                  AND status = 'active'
                "#,
                params![release_status, released_at, reservation_id],
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to release reservation lease: {err}"))
            })?;
        Ok(u64::try_from(changed).unwrap_or(0))
    }

    pub fn expire_offer_reservation_leases(&self, now: Option<DateTime<Utc>>) -> SignerResult<u64> {
        let now_iso = now.unwrap_or_else(Utc::now).to_rfc3339();
        let changed = self
            .conn
            .execute(
                r#"
                UPDATE offer_reservation_lease
                SET status = 'expired',
                    released_at = COALESCE(released_at, ?1)
                WHERE status = 'active'
                  AND expires_at <= ?2
                "#,
                params![now_iso, now_iso],
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to expire reservation leases: {err}"))
            })?;
        Ok(u64::try_from(changed).unwrap_or(0))
    }
}
