use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::error::{SignerError, SignerResult};

use super::{utcnow_iso, SqliteStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferReservationLeaseRow {
    pub reservation_id: String,
    pub market_id: String,
    pub wallet_id: String,
    pub asset_id: String,
    pub amount: i64,
    pub status: String,
    pub created_at: String,
    pub expires_at: String,
    pub released_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferReservationLeaseRequest<'a> {
    pub reservation_id: &'a str,
    pub market_id: &'a str,
    pub wallet_id: &'a str,
    pub requested_amounts: &'a BTreeMap<String, i64>,
    pub available_amounts: &'a BTreeMap<String, i64>,
    pub lease_seconds: i64,
    pub now: Option<DateTime<Utc>>,
}

impl SqliteStore {
    pub fn try_acquire_offer_reservation_lease(
        &self,
        request: &OfferReservationLeaseRequest<'_>,
    ) -> SignerResult<Option<String>> {
        let OfferReservationLeaseRequest {
            reservation_id,
            market_id,
            wallet_id,
            requested_amounts,
            available_amounts,
            lease_seconds,
            now,
        } = *request;
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
            self.conn
                .execute(
                    r"
                UPDATE offer_reservation_lease
                SET status = 'expired',
                    released_at = COALESCE(released_at, ?1)
                WHERE status = 'active'
                  AND expires_at <= ?2
                ",
                    params![now_iso, now_iso],
                )
                .map_err(|err| {
                    SignerError::Other(format!("failed to expire stale reservation leases: {err}"))
                })?;
            let mut stmt = self
                .conn
                .prepare(
                    r"
                SELECT asset_id, COALESCE(SUM(amount), 0) AS reserved_amount
                FROM offer_reservation_lease
                WHERE wallet_id = ?1
                  AND status = 'active'
                  AND expires_at > ?2
                GROUP BY asset_id
                ",
                )
                .map_err(|err| {
                    SignerError::Other(format!("failed to prepare reserved amounts query: {err}"))
                })?;
            let mut rows = stmt.query(params![wallet_id, now_iso]).map_err(|err| {
                SignerError::Other(format!("failed to query reserved amounts: {err}"))
            })?;
            let mut reserved_by_asset: BTreeMap<String, i64> = BTreeMap::default();
            while let Some(row) = rows.next().map_err(|err| {
                SignerError::Other(format!("failed to read reserved amount row: {err}"))
            })? {
                let asset_id: String = row
                    .get(0)
                    .map_err(|err| SignerError::Other(format!("failed to read asset_id: {err}")))?;
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
                    r"
                    INSERT INTO offer_reservation_lease
                      (reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, NULL)
                    ",
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
                r"
                UPDATE offer_reservation_lease
                SET status = ?1, released_at = ?2
                WHERE reservation_id = ?3
                  AND status = 'active'
                ",
                params![release_status, released_at, reservation_id],
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to release reservation lease: {err}"))
            })?;
        super::sqlite_rows_changed(changed)
    }

    pub fn expire_offer_reservation_leases(&self, now: Option<DateTime<Utc>>) -> SignerResult<u64> {
        let now_iso = now.unwrap_or_else(Utc::now).to_rfc3339();
        let changed = self
            .conn
            .execute(
                r"
                UPDATE offer_reservation_lease
                SET status = 'expired',
                    released_at = COALESCE(released_at, ?1)
                WHERE status = 'active'
                  AND expires_at <= ?2
                ",
                params![now_iso, now_iso],
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to expire reservation leases: {err}"))
            })?;
        super::sqlite_rows_changed(changed)
    }

    pub fn add_offer_reservation_lease(
        &self,
        reservation_id: &str,
        market_id: &str,
        wallet_id: &str,
        asset_amounts: &BTreeMap<String, i64>,
        lease_seconds: i64,
    ) -> SignerResult<()> {
        if reservation_id.trim().is_empty() {
            return Err(SignerError::Other("reservation_id is required".to_string()));
        }
        if lease_seconds <= 0 {
            return Err(SignerError::Other("lease_seconds must be > 0".to_string()));
        }
        let created_at = utcnow_iso();
        let expires_at = (Utc::now() + chrono::Duration::seconds(lease_seconds)).to_rfc3339();
        let rows: Vec<(&String, i64)> = asset_amounts
            .iter()
            .filter_map(|(asset_id, amount)| (*amount > 0).then_some((asset_id, *amount)))
            .collect();
        if rows.is_empty() {
            return Err(SignerError::Other(
                "asset_amounts must contain positive amounts".to_string(),
            ));
        }
        for (asset_id, amount) in rows {
            self.conn
                .execute(
                    r"
                    INSERT INTO offer_reservation_lease
                      (reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, NULL)
                    ",
                    params![
                        reservation_id,
                        market_id,
                        wallet_id,
                        asset_id,
                        amount,
                        created_at,
                        expires_at,
                    ],
                )
                .map_err(|err| {
                    SignerError::Other(format!("failed to insert reservation lease: {err}"))
                })?;
        }
        Ok(())
    }

    pub fn list_offer_reservation_leases(
        &self,
        reservation_id: Option<&str>,
    ) -> SignerResult<Vec<OfferReservationLeaseRow>> {
        let mut sql = String::from(
            r"
            SELECT reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at
            FROM offer_reservation_lease
            ",
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(reservation_id) = reservation_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            sql.push_str("WHERE reservation_id = ?1 ");
            params.push(Box::new(reservation_id.to_string()));
        }
        sql.push_str("ORDER BY id ASC");
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(std::convert::AsRef::as_ref).collect();
        let mut stmt = self.conn.prepare(&sql).map_err(|err| {
            SignerError::Other(format!("failed to prepare reservation lease query: {err}"))
        })?;
        let mut rows = stmt.query(param_refs.as_slice()).map_err(|err| {
            SignerError::Other(format!("failed to query reservation leases: {err}"))
        })?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|err| {
            SignerError::Other(format!("failed to read reservation lease row: {err}"))
        })? {
            out.push(OfferReservationLeaseRow {
                reservation_id: row.get(0).map_err(|err| {
                    SignerError::Other(format!("failed to read reservation_id: {err}"))
                })?,
                market_id: row.get(1).map_err(|err| {
                    SignerError::Other(format!("failed to read market_id: {err}"))
                })?,
                wallet_id: row.get(2).map_err(|err| {
                    SignerError::Other(format!("failed to read wallet_id: {err}"))
                })?,
                asset_id: row
                    .get(3)
                    .map_err(|err| SignerError::Other(format!("failed to read asset_id: {err}")))?,
                amount: row
                    .get(4)
                    .map_err(|err| SignerError::Other(format!("failed to read amount: {err}")))?,
                status: row
                    .get(5)
                    .map_err(|err| SignerError::Other(format!("failed to read status: {err}")))?,
                created_at: row.get(6).map_err(|err| {
                    SignerError::Other(format!("failed to read created_at: {err}"))
                })?,
                expires_at: row.get(7).map_err(|err| {
                    SignerError::Other(format!("failed to read expires_at: {err}"))
                })?,
                released_at: row.get(8).ok(),
            });
        }
        Ok(out)
    }

    pub fn get_offer_reserved_amounts_by_asset(
        &self,
        wallet_id: &str,
    ) -> SignerResult<BTreeMap<String, i64>> {
        let now_iso = utcnow_iso();
        let mut stmt = self
            .conn
            .prepare(
                r"
                SELECT asset_id, COALESCE(SUM(amount), 0) AS reserved_amount
                FROM offer_reservation_lease
                WHERE wallet_id = ?1
                  AND status = 'active'
                  AND expires_at > ?2
                GROUP BY asset_id
                ",
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to prepare reserved amounts query: {err}"))
            })?;
        let mut rows = stmt.query(params![wallet_id, now_iso]).map_err(|err| {
            SignerError::Other(format!("failed to query reserved amounts: {err}"))
        })?;
        let mut out = BTreeMap::default();
        while let Some(row) = rows.next().map_err(|err| {
            SignerError::Other(format!("failed to read reserved amount row: {err}"))
        })? {
            let asset_id: String = row
                .get(0)
                .map_err(|err| SignerError::Other(format!("failed to read asset_id: {err}")))?;
            let amount: i64 = row.get(1).map_err(|err| {
                SignerError::Other(format!("failed to read reserved_amount: {err}"))
            })?;
            out.insert(asset_id, amount);
        }
        Ok(out)
    }

    pub fn prune_offer_reservation_leases(&self, older_than: DateTime<Utc>) -> SignerResult<u64> {
        let cutoff_iso = older_than.to_rfc3339();
        let changed = self
            .conn
            .execute(
                r"
                DELETE FROM offer_reservation_lease
                WHERE status != 'active'
                  AND COALESCE(released_at, expires_at) < ?1
                ",
                params![cutoff_iso],
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to prune reservation leases: {err}"))
            })?;
        super::sqlite_rows_changed(changed)
    }
}
