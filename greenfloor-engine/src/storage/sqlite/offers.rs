use std::collections::HashMap;

use crate::cycle::{OfferLifecycleState, ReconcileState};
use crate::error::{SignerError, SignerResult};
use rusqlite::params;

use super::{utcnow_iso, OfferStateDetailRow, OfferStateListRow, SqliteStore};

fn read_offer_state_list_row(row: &rusqlite::Row<'_>) -> SignerResult<OfferStateListRow> {
    Ok(OfferStateListRow {
        offer_id: row
            .get(0)
            .map_err(|err| SignerError::Other(format!("failed to read offer_id: {err}")))?,
        market_id: row
            .get(1)
            .map_err(|err| SignerError::Other(format!("failed to read market_id: {err}")))?,
        state: row
            .get(2)
            .map_err(|err| SignerError::Other(format!("failed to read state: {err}")))?,
        last_seen_status: row.get(3).ok(),
        updated_at: row
            .get(4)
            .map_err(|err| SignerError::Other(format!("failed to read updated_at: {err}")))?,
        cancel_submitted_tx_id: row.get(5).ok(),
        cancel_submitted_at: row.get(6).ok(),
        publish_venue: row
            .get::<_, Option<String>>(7)
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty()),
    })
}

const OFFER_STATE_LIST_COLUMNS: &str = "offer_id, market_id, state, last_seen_status, updated_at, cancel_submitted_tx_id, cancel_submitted_at, publish_venue";

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

    /// Upsert offer state using a typed reconcile state.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn upsert_offer_reconcile_state(
        &self,
        offer_id: &str,
        market_id: &str,
        state: &ReconcileState,
        last_seen_status: Option<i64>,
    ) -> SignerResult<()> {
        self.upsert_offer_state(offer_id, market_id, &state.as_str(), last_seen_status)
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
            .prepare(&format!(
                r"
                SELECT {OFFER_STATE_LIST_COLUMNS}
                FROM offer_state
                WHERE state IN (?1, ?2)
                ORDER BY offer_id ASC
                LIMIT ?3 OFFSET ?4
                "
            ))
            .map_err(|err| {
                SignerError::Other(format!("failed to prepare open offer_state query: {err}"))
            })?;
        let open_state = ReconcileState::Lifecycle(OfferLifecycleState::Open);
        let pending_state = ReconcileState::PendingVisibility;
        let mut rows = stmt
            .query(params![
                open_state.as_str(),
                pending_state.as_str(),
                limit_i64,
                offset
            ])
            .map_err(|err| {
                SignerError::Other(format!("failed to query open offer_state: {err}"))
            })?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|err| {
            SignerError::Other(format!("failed to read open offer_state row: {err}"))
        })? {
            out.push(read_offer_state_list_row(row)?);
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
        let clean_ids: Vec<String> = offer_ids
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        if clean_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: Vec<String> =
            (1..=clean_ids.len()).map(|idx| format!("?{idx}")).collect();
        let query = format!(
            r"
            SELECT {OFFER_STATE_LIST_COLUMNS}
            FROM offer_state
            WHERE offer_id IN ({})
            ",
            placeholders.join(", ")
        );
        let mut stmt = self.conn.prepare(&query).map_err(|err| {
            SignerError::Other(format!("failed to prepare offer_state by ids query: {err}"))
        })?;
        let mut rows = stmt
            .query(rusqlite::params_from_iter(clean_ids.iter()))
            .map_err(|err| {
                SignerError::Other(format!("failed to query offer_state by ids: {err}"))
            })?;
        let mut by_id = HashMap::new();
        while let Some(row) = rows.next().map_err(|err| {
            SignerError::Other(format!("failed to read offer_state by ids row: {err}"))
        })? {
            let offer_id: String = row
                .get(0)
                .map_err(|err| SignerError::Other(format!("failed to read offer_id: {err}")))?;
            by_id.insert(offer_id.clone(), read_offer_state_list_row(row)?);
        }
        Ok(clean_ids
            .into_iter()
            .filter_map(|offer_id| by_id.get(&offer_id).cloned())
            .collect())
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
                .prepare(&format!(
                    r"
                SELECT {OFFER_STATE_LIST_COLUMNS}
                FROM offer_state
                WHERE market_id = ?1
                ORDER BY updated_at DESC
                LIMIT ?2
                "
                ))
                .map_err(|err| {
                    SignerError::Other(format!("failed to prepare offer_state query: {err}"))
                })?;
            let mut rows = stmt
                .query(params![market_id, limit_i64])
                .map_err(|err| SignerError::Other(format!("failed to query offer_state: {err}")))?;
            while let Some(row) = rows.next().map_err(|err| {
                SignerError::Other(format!("failed to read offer_state row: {err}"))
            })? {
                out.push(read_offer_state_list_row(row)?);
            }
        } else {
            let mut stmt = self
                .conn
                .prepare(&format!(
                    r"
                SELECT {OFFER_STATE_LIST_COLUMNS}
                FROM offer_state
                ORDER BY updated_at DESC
                LIMIT ?1
                "
                ))
                .map_err(|err| {
                    SignerError::Other(format!("failed to prepare offer_state query: {err}"))
                })?;
            let mut rows = stmt
                .query(params![limit_i64])
                .map_err(|err| SignerError::Other(format!("failed to query offer_state: {err}")))?;
            while let Some(row) = rows.next().map_err(|err| {
                SignerError::Other(format!("failed to read offer_state row: {err}"))
            })? {
                out.push(read_offer_state_list_row(row)?);
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
            super::offer_cancel::OfferCancelWrite::default(),
        )
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
