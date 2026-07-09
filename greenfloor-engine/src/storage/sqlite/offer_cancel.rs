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
    /// Publish venue (`coinset` / `dexie` / `splash`); set at post time only.
    pub publish_venue: Option<&'a str>,
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
    explicit: Option<&str>,
) -> SignerResult<Option<String>> {
    if state != "cancel_submitted" {
        return Ok(None);
    }
    if let Some(value) = explicit {
        return Ok(Some(value.to_string()));
    }
    Ok(conn
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
        .flatten())
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
                  cancel_submitted_at,
                  publish_venue
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
                  cancel_submitted_at = CASE
                    WHEN excluded.state = 'cancel_submitted'
                      THEN COALESCE(excluded.cancel_submitted_at, offer_state.cancel_submitted_at)
                    ELSE NULL
                  END,
                  publish_venue = COALESCE(excluded.publish_venue, offer_state.publish_venue)
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
                    cancel.publish_venue,
                ],
            )
            .map_err(|err| SignerError::Other(format!("failed to upsert offer_state: {err}")))?;
        Ok(())
    }

    /// Whether Dexie is authoritative for missing-offer / 404 lifecycle decisions.
    ///
    /// Explicit `dexie` → yes; anything else (including legacy `NULL` after venue
    /// backfill) → no. Prefer persisted `publish_venue` over id-shape heuristics.
    #[must_use]
    pub fn is_dexie_authoritative_for_offer(_offer_id: &str, publish_venue: Option<&str>) -> bool {
        publish_venue
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some_and(|venue| venue.eq_ignore_ascii_case("dexie"))
    }

    /// Publish venue recorded at post time (`coinset` / `dexie` / `splash`).
    ///
    /// Returns `None` when the offer is absent or venue was never persisted (legacy rows).
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn offer_publish_venue_for_id(&self, offer_id: &str) -> SignerResult<Option<String>> {
        let clean = offer_id.trim();
        if clean.is_empty() {
            return Ok(None);
        }
        self.conn
            .query_row(
                "SELECT publish_venue FROM offer_state WHERE offer_id = ?1",
                params![clean],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|err| {
                SignerError::Other(format!(
                    "failed to read offer_state publish_venue for {clean}: {err}"
                ))
            })
            .map(|row| row.flatten().filter(|value| !value.trim().is_empty()))
    }

    /// Whether Dexie missing-offer / 404 should drive lifecycle for this offer id.
    ///
    /// # Errors
    ///
    /// Returns an error if the venue lookup fails.
    pub fn is_dexie_authoritative_offer(&self, offer_id: &str) -> SignerResult<bool> {
        let venue = self.offer_publish_venue_for_id(offer_id)?;
        Ok(Self::is_dexie_authoritative_for_offer(
            offer_id,
            venue.as_deref(),
        ))
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

    /// Persist `cancel_submitted` and the cancel tx id **without** observing the cancel
    /// tx. Watches stay registered so stale unwedge back to `open` keeps coin-ops
    /// protection; terminal persist clears them. After successful broadcast, observe the
    /// cancel tx via [`crate::storage::TxSignalIngress::Mempool`].
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn prepare_offer_cancel_submitted(
        &self,
        offer_id: &str,
        market_id: &str,
        cancel_tx_id: &str,
        last_seen_status: Option<i64>,
    ) -> SignerResult<()> {
        self.prepare_offer_cancel_submitted_at(
            offer_id,
            market_id,
            cancel_tx_id,
            last_seen_status,
            &super::utcnow_iso(),
        )
    }

    pub(crate) fn prepare_offer_cancel_submitted_at(
        &self,
        offer_id: &str,
        market_id: &str,
        cancel_tx_id: &str,
        last_seen_status: Option<i64>,
        updated_at: &str,
    ) -> SignerResult<()> {
        let stored_cancel_tx_id = canonical_tx_id(cancel_tx_id)
            .ok_or_else(|| SignerError::Other(format!("invalid cancel tx id: {cancel_tx_id}")))?;
        self.immediate_transaction("cancel_submitted_prepare", |store| {
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
            Ok(())
        })
    }

    /// Roll back a prepared (pre-broadcast) cancel submit to `prior_state`.
    /// Watches are left intact (prepare never clears them).
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn rollback_offer_cancel_submitted(
        &self,
        offer_id: &str,
        market_id: &str,
        prior_state: &str,
    ) -> SignerResult<()> {
        self.upsert_offer_state(offer_id, market_id, prior_state, None)
    }

    /// Persist committed `cancel_submitted` (prepare + observe cancel tx) for tests.
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
        self.prepare_offer_cancel_submitted(offer_id, market_id, cancel_tx_id, last_seen_status)?;
        let stored_cancel_tx_id = canonical_tx_id(cancel_tx_id)
            .ok_or_else(|| SignerError::Other(format!("invalid cancel tx id: {cancel_tx_id}")))?;
        self.ingest_tx_signals(
            std::slice::from_ref(&stored_cancel_tx_id),
            crate::storage::TxSignalIngress::Mempool,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod venue_authority_tests {
    use super::SqliteStore;

    #[test]
    fn dexie_authority_uses_explicit_venue_only() {
        assert!(SqliteStore::is_dexie_authoritative_for_offer(
            "offer-bech32-like",
            Some("dexie")
        ));
        assert!(!SqliteStore::is_dexie_authoritative_for_offer(
            &"ab".repeat(32),
            Some("coinset")
        ));
        assert!(!SqliteStore::is_dexie_authoritative_for_offer(
            &"ab".repeat(32),
            None
        ));
        assert!(!SqliteStore::is_dexie_authoritative_for_offer(
            "legacy-dexie-id",
            None
        ));
        assert!(!SqliteStore::is_dexie_authoritative_for_offer(
            "legacy-dexie-id",
            Some("splash")
        ));
    }
}
