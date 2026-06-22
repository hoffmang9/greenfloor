use crate::error::{SignerError, SignerResult};
use crate::offer::types::{OfferExecutionMode, PresplitCancelFields, StoredOfferCancelMetadata};
use rusqlite::params;

use super::SqliteStore;

/// Cancel metadata written alongside an offer state upsert.
#[derive(Debug, Clone, Copy, Default)]
pub struct OfferCancelWrite<'a> {
    pub fields: Option<&'a PresplitCancelFields>,
    pub execution_mode: Option<OfferExecutionMode>,
}

pub(crate) fn cancel_metadata_params(
    cancel: OfferCancelWrite<'_>,
) -> (
    Option<&str>,
    Option<&str>,
    Option<String>,
) {
    let execution_mode_str = cancel.execution_mode.map(|mode| mode.to_string());
    if let Some(fields) = cancel.fields {
        (
            fields.input_coin_id.as_deref(),
            fields.fixed_delegated_puzzle_hash.as_deref(),
            execution_mode_str,
        )
    } else {
        (None, None, execution_mode_str)
    }
}

impl SqliteStore {
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
        cancel: OfferCancelWrite<'_>,
    ) -> SignerResult<()> {
        let (presplit_input_coin_id, fixed_delegated_puzzle_hash, execution_mode_str) =
            cancel_metadata_params(cancel);
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
                    execution_mode_str.as_deref(),
                ],
            )
            .map_err(|err| SignerError::Other(format!("failed to upsert offer_state: {err}")))?;
        Ok(())
    }

    /// Load cancel metadata persisted at offer post time.
    ///
    /// Returns `None` when the offer id is absent from `offer_state`.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn offer_cancel_metadata_for_id(
        &self,
        offer_id: &str,
    ) -> SignerResult<Option<StoredOfferCancelMetadata>> {
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
                    "failed to prepare offer cancel fields query: {err}"
                ))
            })?;
        let mut rows = stmt.query(params![clean]).map_err(|err| {
            SignerError::Other(format!("failed to query offer cancel fields: {err}"))
        })?;
        let Some(row) = rows.next().map_err(|err| {
            SignerError::Other(format!("failed to read offer cancel fields row: {err}"))
        })?
        else {
            return Ok(None);
        };
        Ok(Some(StoredOfferCancelMetadata {
            fields: PresplitCancelFields {
                input_coin_id: row.get(0).ok(),
                fixed_delegated_puzzle_hash: row.get(1).ok(),
            },
            execution_mode: row
                .get::<_, Option<String>>(2)
                .ok()
                .flatten()
                .and_then(|value| OfferExecutionMode::parse_db(&value)),
        }))
    }
}
