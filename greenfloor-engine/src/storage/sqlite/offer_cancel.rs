use crate::error::{SignerError, SignerResult};
use crate::hex::canonical_tx_id;
use crate::offer::types::{OfferExecutionMode, PresplitCancelFields, StoredOfferCancelMetadata};
use rusqlite::{params, OptionalExtension};

use super::SqliteStore;

/// Cancel metadata written alongside an offer state upsert.
#[derive(Debug, Clone, Copy, Default)]
pub struct OfferCancelWrite<'a> {
    pub fields: Option<&'a PresplitCancelFields>,
    pub execution_mode: Option<OfferExecutionMode>,
    pub cancel_submitted_tx_id: Option<&'a str>,
    pub cancel_submitted_at: Option<&'a str>,
}

pub(crate) fn cancel_metadata_params(
    cancel: OfferCancelWrite<'_>,
) -> (Option<&str>, Option<&str>, Option<String>) {
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

fn resolve_cancel_submitted_at_for_upsert(
    conn: &rusqlite::Connection,
    offer_id: &str,
    state: &str,
    updated_at: &str,
    explicit: Option<&str>,
) -> SignerResult<Option<String>> {
    if state != "cancel_submitted" {
        return Ok(None);
    }
    if let Some(value) = explicit {
        return Ok(Some(value.to_string()));
    }
    let existing = conn
        .query_row(
            "SELECT cancel_submitted_at FROM offer_state WHERE offer_id = ?1",
            params![offer_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|err| {
            SignerError::Other(format!(
                "failed to read offer_state cancel_submitted_at for {offer_id}: {err}"
            ))
        })?
        .flatten();
    Ok(existing.or_else(|| Some(updated_at.to_string())))
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
        let cancel_submitted_at = resolve_cancel_submitted_at_for_upsert(
            &self.conn,
            offer_id,
            state,
            updated_at,
            cancel.cancel_submitted_at,
        )?;
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
                  execution_mode,
                  cancel_submitted_tx_id,
                  cancel_submitted_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(offer_id) DO UPDATE SET
                  market_id = excluded.market_id,
                  state = excluded.state,
                  last_seen_status = excluded.last_seen_status,
                  updated_at = excluded.updated_at,
                  presplit_input_coin_id = COALESCE(excluded.presplit_input_coin_id, offer_state.presplit_input_coin_id),
                  fixed_delegated_puzzle_hash = COALESCE(excluded.fixed_delegated_puzzle_hash, offer_state.fixed_delegated_puzzle_hash),
                  execution_mode = COALESCE(excluded.execution_mode, offer_state.execution_mode),
                  cancel_submitted_tx_id = CASE
                    WHEN excluded.state = 'cancel_submitted'
                      THEN COALESCE(excluded.cancel_submitted_tx_id, offer_state.cancel_submitted_tx_id)
                    ELSE NULL
                  END,
                  cancel_submitted_at = excluded.cancel_submitted_at
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
                    cancel.cancel_submitted_tx_id,
                    cancel_submitted_at,
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

    /// Persist `cancel_submitted` and the submitted cancel transaction id.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn upsert_offer_cancel_submitted(
        &self,
        offer_id: &str,
        market_id: &str,
        cancel_tx_id: &str,
        last_seen_status: Option<i64>,
    ) -> SignerResult<()> {
        self.upsert_offer_cancel_submitted_at(
            offer_id,
            market_id,
            cancel_tx_id,
            last_seen_status,
            &super::utcnow_iso(),
        )
    }

    pub(crate) fn upsert_offer_cancel_submitted_at(
        &self,
        offer_id: &str,
        market_id: &str,
        cancel_tx_id: &str,
        last_seen_status: Option<i64>,
        updated_at: &str,
    ) -> SignerResult<()> {
        let stored_cancel_tx_id = canonical_tx_id(cancel_tx_id)
            .ok_or_else(|| SignerError::Other(format!("invalid cancel tx id: {cancel_tx_id}")))?;
        self.immediate_transaction("cancel_submitted", |store| {
            store.upsert_offer_state_with_metadata_at(
                offer_id,
                market_id,
                "cancel_submitted",
                last_seen_status,
                updated_at,
                OfferCancelWrite {
                    cancel_submitted_tx_id: Some(stored_cancel_tx_id.as_str()),
                    cancel_submitted_at: Some(updated_at),
                    ..OfferCancelWrite::default()
                },
            )?;
            store.observe_mempool_tx_ids(std::slice::from_ref(&stored_cancel_tx_id))?;
            Ok(())
        })
    }
}
