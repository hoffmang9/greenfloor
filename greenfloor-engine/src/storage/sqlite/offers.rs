use crate::error::{SignerError, SignerResult};
use rusqlite::params;

use super::{utcnow_iso, OfferStateDetailRow, OfferStateListRow, SqliteStore};

impl SqliteStore {
    /// Upsert offer state.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn upsert_offer_state(
        &self,
        offer_id: &str,
        market_id: &str,
        state: &str,
        last_seen_status: Option<i64>,
    ) -> SignerResult<()> {
        self.upsert_offer_state_at(offer_id, market_id, state, last_seen_status, &utcnow_iso())
    }

    /// List open (or pending visibility) offer states for cancel-open flows.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_open_offer_states(&self, limit: usize) -> SignerResult<Vec<OfferStateListRow>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit_i64 = i64::try_from(limit).map_err(|_| {
            SignerError::Other("list_open_offer_states limit exceeds i64 max".to_string())
        })?;
        let mut stmt = self
            .conn
            .prepare(
                r"
                SELECT offer_id, market_id, state, last_seen_status, updated_at
                FROM offer_state
                WHERE state IN ('open', 'pending_visibility')
                ORDER BY updated_at DESC
                LIMIT ?1
                ",
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to prepare open offer_state query: {err}"))
            })?;
        let mut rows = stmt.query(params![limit_i64]).map_err(|err| {
            SignerError::Other(format!("failed to query open offer_state: {err}"))
        })?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|err| {
            SignerError::Other(format!("failed to read open offer_state row: {err}"))
        })? {
            out.push(OfferStateListRow {
                offer_id: row
                    .get(0)
                    .map_err(|err| SignerError::Other(format!("failed to read offer_id: {err}")))?,
                market_id: row.get(1).map_err(|err| {
                    SignerError::Other(format!("failed to read market_id: {err}"))
                })?,
                state: row
                    .get(2)
                    .map_err(|err| SignerError::Other(format!("failed to read state: {err}")))?,
                last_seen_status: row.get(3).ok(),
                updated_at: row.get(4).map_err(|err| {
                    SignerError::Other(format!("failed to read updated_at: {err}"))
                })?,
            });
        }
        Ok(out)
    }

    /// List offer states for explicit offer ids (order follows input ids).
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_offer_states_for_ids(
        &self,
        offer_ids: &[String],
    ) -> SignerResult<Vec<OfferStateListRow>> {
        let mut out = Vec::new();
        for offer_id in offer_ids {
            let clean = offer_id.trim();
            if clean.is_empty() {
                continue;
            }
            let mut stmt = self
                .conn
                .prepare(
                    r"
                    SELECT offer_id, market_id, state, last_seen_status, updated_at
                    FROM offer_state
                    WHERE offer_id = ?1
                    ",
                )
                .map_err(|err| {
                    SignerError::Other(format!("failed to prepare offer_state by id query: {err}"))
                })?;
            let mut rows = stmt.query(params![clean]).map_err(|err| {
                SignerError::Other(format!("failed to query offer_state by id: {err}"))
            })?;
            if let Some(row) = rows.next().map_err(|err| {
                SignerError::Other(format!("failed to read offer_state by id row: {err}"))
            })? {
                out.push(OfferStateListRow {
                    offer_id: row.get(0).map_err(|err| {
                        SignerError::Other(format!("failed to read offer_id: {err}"))
                    })?,
                    market_id: row.get(1).map_err(|err| {
                        SignerError::Other(format!("failed to read market_id: {err}"))
                    })?,
                    state: row.get(2).map_err(|err| {
                        SignerError::Other(format!("failed to read state: {err}"))
                    })?,
                    last_seen_status: row.get(3).ok(),
                    updated_at: row.get(4).map_err(|err| {
                        SignerError::Other(format!("failed to read updated_at: {err}"))
                    })?,
                });
            }
        }
        Ok(out)
    }

    /// List offer states.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_offer_states(
        &self,
        market_id: Option<&str>,
        limit: usize,
    ) -> SignerResult<Vec<OfferStateListRow>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit_i64 = i64::try_from(limit).map_err(|_| {
            SignerError::Other("list_offer_states limit exceeds i64 max".to_string())
        })?;
        let mut out = Vec::new();
        if let Some(market_id) = market_id.map(str::trim).filter(|value| !value.is_empty()) {
            let mut stmt = self
                .conn
                .prepare(
                    r"
                SELECT offer_id, market_id, state, last_seen_status, updated_at
                FROM offer_state
                WHERE market_id = ?1
                ORDER BY updated_at DESC
                LIMIT ?2
                ",
                )
                .map_err(|err| {
                    SignerError::Other(format!("failed to prepare offer_state query: {err}"))
                })?;
            let mut rows = stmt
                .query(params![market_id, limit_i64])
                .map_err(|err| SignerError::Other(format!("failed to query offer_state: {err}")))?;
            while let Some(row) = rows.next().map_err(|err| {
                SignerError::Other(format!("failed to read offer_state row: {err}"))
            })? {
                out.push(OfferStateListRow {
                    offer_id: row.get(0).map_err(|err| {
                        SignerError::Other(format!("failed to read offer_id: {err}"))
                    })?,
                    market_id: row.get(1).map_err(|err| {
                        SignerError::Other(format!("failed to read market_id: {err}"))
                    })?,
                    state: row.get(2).map_err(|err| {
                        SignerError::Other(format!("failed to read state: {err}"))
                    })?,
                    last_seen_status: row.get(3).ok(),
                    updated_at: row.get(4).map_err(|err| {
                        SignerError::Other(format!("failed to read updated_at: {err}"))
                    })?,
                });
            }
        } else {
            let mut stmt = self
                .conn
                .prepare(
                    r"
                SELECT offer_id, market_id, state, last_seen_status, updated_at
                FROM offer_state
                ORDER BY updated_at DESC
                LIMIT ?1
                ",
                )
                .map_err(|err| {
                    SignerError::Other(format!("failed to prepare offer_state query: {err}"))
                })?;
            let mut rows = stmt
                .query(params![limit_i64])
                .map_err(|err| SignerError::Other(format!("failed to query offer_state: {err}")))?;
            while let Some(row) = rows.next().map_err(|err| {
                SignerError::Other(format!("failed to read offer_state row: {err}"))
            })? {
                out.push(OfferStateListRow {
                    offer_id: row.get(0).map_err(|err| {
                        SignerError::Other(format!("failed to read offer_id: {err}"))
                    })?,
                    market_id: row.get(1).map_err(|err| {
                        SignerError::Other(format!("failed to read market_id: {err}"))
                    })?,
                    state: row.get(2).map_err(|err| {
                        SignerError::Other(format!("failed to read state: {err}"))
                    })?,
                    last_seen_status: row.get(3).ok(),
                    updated_at: row.get(4).map_err(|err| {
                        SignerError::Other(format!("failed to read updated_at: {err}"))
                    })?,
                });
            }
        }
        Ok(out)
    }

    /// List offer state details.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_offer_state_details(
        &self,
        market_id: &str,
        limit: usize,
    ) -> SignerResult<Vec<OfferStateDetailRow>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit_i64 = i64::try_from(limit).map_err(|_| {
            SignerError::Other("list_offer_state_details limit exceeds i64 max".to_string())
        })?;
        let mut stmt = self
            .conn
            .prepare(
                r"
            SELECT offer_id, market_id, state, last_seen_status, updated_at
            FROM offer_state
            WHERE market_id = ?1
            ORDER BY updated_at DESC
            LIMIT ?2
            ",
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to prepare offer_state detail query: {err}"))
            })?;
        let mut rows = stmt.query(params![market_id, limit_i64]).map_err(|err| {
            SignerError::Other(format!("failed to query offer_state details: {err}"))
        })?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|err| {
            SignerError::Other(format!("failed to read offer_state detail row: {err}"))
        })? {
            out.push(OfferStateDetailRow {
                offer_id: row
                    .get(0)
                    .map_err(|err| SignerError::Other(format!("failed to read offer_id: {err}")))?,
                market_id: row.get(1).map_err(|err| {
                    SignerError::Other(format!("failed to read market_id: {err}"))
                })?,
                state: row
                    .get(2)
                    .map_err(|err| SignerError::Other(format!("failed to read state: {err}")))?,
                last_seen_status: row.get(3).ok(),
                updated_at: row.get(4).map_err(|err| {
                    SignerError::Other(format!("failed to read updated_at: {err}"))
                })?,
            });
        }
        Ok(out)
    }

    /// Upsert offer state at.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn upsert_offer_state_at(
        &self,
        offer_id: &str,
        market_id: &str,
        state: &str,
        last_seen_status: Option<i64>,
        updated_at: &str,
    ) -> SignerResult<()> {
        self.conn
            .execute(
                r"
                INSERT INTO offer_state (offer_id, market_id, state, last_seen_status, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(offer_id) DO UPDATE SET
                  market_id = excluded.market_id,
                  state = excluded.state,
                  last_seen_status = excluded.last_seen_status,
                  updated_at = excluded.updated_at
                ",
                params![offer_id, market_id, state, last_seen_status, updated_at,],
            )
            .map_err(|err| SignerError::Other(format!("failed to upsert offer_state: {err}")))?;
        Ok(())
    }
}

#[cfg(test)]
impl SqliteStore {
    pub(crate) fn offer_state_for_id(&self, offer_id: &str) -> SignerResult<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT state FROM offer_state WHERE offer_id = ?1")
            .map_err(|err| {
                SignerError::Other(format!("failed to prepare offer_state query: {err}"))
            })?;
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
}
