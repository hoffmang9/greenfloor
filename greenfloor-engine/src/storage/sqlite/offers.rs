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

    /// List a page of open (or pending visibility) offer states.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_open_offer_states_page(
        &self,
        limit: usize,
        offset: i64,
    ) -> SignerResult<Vec<OfferStateListRow>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit_i64 = i64::try_from(limit).map_err(|_| {
            SignerError::Other("list_open_offer_states_page limit exceeds i64 max".to_string())
        })?;
        let mut stmt = self
            .conn
            .prepare(
                r"
                SELECT offer_id, market_id, state, last_seen_status, updated_at
                FROM offer_state
                WHERE state IN ('open', 'pending_visibility')
                ORDER BY offer_id ASC
                LIMIT ?1 OFFSET ?2
                ",
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to prepare open offer_state query: {err}"))
            })?;
        let mut rows = stmt.query(params![limit_i64, offset]).map_err(|err| {
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

    /// List all open (or pending visibility) offer states without recency bias.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_all_open_offer_states(&self) -> SignerResult<Vec<OfferStateListRow>> {
        const PAGE_SIZE: usize = 1_000;
        let mut all = Vec::new();
        let mut offset = 0_i64;
        loop {
            let page = self.list_open_offer_states_page(PAGE_SIZE, offset)?;
            if page.is_empty() {
                break;
            }
            let count = i64::try_from(page.len()).map_err(|_| {
                SignerError::Other("open offer_state page length exceeds i64 max".to_string())
            })?;
            all.extend(page);
            if usize::try_from(count).map_err(|_| {
                SignerError::Other("open offer_state page length exceeds usize max".to_string())
            })? < PAGE_SIZE
            {
                break;
            }
            offset = offset.saturating_add(count);
        }
        Ok(all)
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
        self.upsert_offer_state_with_metadata_at(
            offer_id,
            market_id,
            state,
            last_seen_status,
            updated_at,
            None,
        )
    }

    /// Upsert offer state and optional presplit cancel metadata captured at post time.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn upsert_offer_state_with_metadata_at(
        &self,
        offer_id: &str,
        market_id: &str,
        state: &str,
        last_seen_status: Option<i64>,
        updated_at: &str,
        cancel_metadata: Option<&super::OfferCancelMetadataRow>,
    ) -> SignerResult<()> {
        let (presplit_input_coin_id, fixed_delegated_puzzle_hash, execution_mode) =
            if let Some(metadata) = cancel_metadata {
                (
                    metadata.presplit_input_coin_id.as_deref(),
                    metadata.fixed_delegated_puzzle_hash.as_deref(),
                    metadata.execution_mode.as_deref(),
                )
            } else {
                (None, None, None)
            };
        self.conn
            .execute(
                r"
                INSERT INTO offer_state (
                  offer_id,
                  market_id,
                  state,
                  last_seen_status,
                  updated_at,
                  presplit_input_coin_id,
                  fixed_delegated_puzzle_hash,
                  execution_mode
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(offer_id) DO UPDATE SET
                  market_id = excluded.market_id,
                  state = excluded.state,
                  last_seen_status = excluded.last_seen_status,
                  updated_at = excluded.updated_at,
                  presplit_input_coin_id = COALESCE(excluded.presplit_input_coin_id, offer_state.presplit_input_coin_id),
                  fixed_delegated_puzzle_hash = COALESCE(excluded.fixed_delegated_puzzle_hash, offer_state.fixed_delegated_puzzle_hash),
                  execution_mode = COALESCE(excluded.execution_mode, offer_state.execution_mode)
                ",
                params![
                    offer_id,
                    market_id,
                    state,
                    last_seen_status,
                    updated_at,
                    presplit_input_coin_id,
                    fixed_delegated_puzzle_hash,
                    execution_mode,
                ],
            )
            .map_err(|err| SignerError::Other(format!("failed to upsert offer_state: {err}")))?;
        Ok(())
    }

    /// Load presplit cancel metadata persisted at offer post time.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn offer_cancel_metadata_for_id(
        &self,
        offer_id: &str,
    ) -> SignerResult<Option<super::OfferCancelMetadataRow>> {
        let clean = offer_id.trim();
        if clean.is_empty() {
            return Ok(None);
        }
        let mut stmt = self
            .conn
            .prepare(
                r"
                SELECT presplit_input_coin_id, fixed_delegated_puzzle_hash, execution_mode
                FROM offer_state
                WHERE offer_id = ?1
                ",
            )
            .map_err(|err| {
                SignerError::Other(format!(
                    "failed to prepare offer cancel metadata query: {err}"
                ))
            })?;
        let mut rows = stmt.query(params![clean]).map_err(|err| {
            SignerError::Other(format!("failed to query offer cancel metadata: {err}"))
        })?;
        let Some(row) = rows.next().map_err(|err| {
            SignerError::Other(format!("failed to read offer cancel metadata row: {err}"))
        })?
        else {
            return Ok(None);
        };
        Ok(Some(super::OfferCancelMetadataRow {
            presplit_input_coin_id: row.get(0).ok(),
            fixed_delegated_puzzle_hash: row.get(1).ok(),
            execution_mode: row.get(2).ok(),
        }))
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
