use std::collections::{HashMap, HashSet};

use crate::cycle::reconcile::{BINDING_MAKER_QUERY_STATES, LADDER_CAPACITY_QUERY_STATES};
use crate::cycle::{OfferLifecycleState, ReconcileState};
use crate::error::{SignerError, SignerResult};
use crate::hex::{canonical_tx_id, normalize_hex_id};
use rusqlite::{params, OptionalExtension};

use super::{db_err, in_placeholders, query_mapped, utcnow_iso, OfferStateListRow, SqliteStore};

pub(crate) fn read_offer_state_list_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<OfferStateListRow> {
    Ok(OfferStateListRow {
        offer_id: row.get(0)?,
        market_id: row.get(1)?,
        state: row.get(2)?,
        last_seen_status: row.get(3)?,
        updated_at: row.get(4)?,
        cancel_submitted_tx_id: row.get(5)?,
        cancel_submitted_at: row.get(6)?,
        publish_venue: row
            .get::<_, Option<String>>(7)?
            .filter(|value| !value.trim().is_empty()),
    })
}

pub(crate) const OFFER_STATE_LIST_COLUMNS: &str = "offer_id, market_id, state, last_seen_status, updated_at, cancel_submitted_tx_id, cancel_submitted_at, publish_venue";

/// Qualify [`OFFER_STATE_LIST_COLUMNS`] with `alias.` for JOINs (same order as the reader).
pub(crate) fn offer_state_list_columns_aliased(alias: &str) -> String {
    OFFER_STATE_LIST_COLUMNS
        .split(", ")
        .map(|col| format!("{alias}.{col}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn limit_i64(limit: usize, context: &str) -> SignerResult<Option<i64>> {
    if limit == 0 {
        return Ok(None);
    }
    i64::try_from(limit)
        .map(Some)
        .map_err(|_| SignerError::Other(format!("{context} limit exceeds i64 max")))
}

impl SqliteStore {
    /// `where_template` must contain `{in}` for the numbered `IN (...)` placeholders.
    fn query_offer_state_list_by_ids(
        &self,
        ids: &[String],
        where_template: &str,
        context: &str,
    ) -> SignerResult<Vec<OfferStateListRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT {OFFER_STATE_LIST_COLUMNS} FROM offer_state WHERE {where}",
            where = where_template.replace("{in}", &in_placeholders(ids.len())),
        );
        query_mapped(
            &self.conn,
            &sql,
            rusqlite::params_from_iter(ids.iter()),
            context,
            read_offer_state_list_row,
        )
    }

    /// Upsert offer state.
    ///
    /// `state` must be a known [`ReconcileState`]. Use [`Self::upsert_offer_state_at`]
    /// only for timestamped writes of an already-validated state string.
    ///
    /// # Errors
    ///
    /// Returns an error if `state` is unknown or the write fails.
    pub fn upsert_offer_state(
        &self,
        offer_id: &str,
        market_id: &str,
        state: &str,
        last_seen_status: Option<i64>,
    ) -> SignerResult<()> {
        let parsed = ReconcileState::parse(state)?;
        self.upsert_offer_reconcile_state(offer_id, market_id, &parsed, last_seen_status)
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
        self.upsert_offer_state_at(
            offer_id,
            market_id,
            &state.as_str(),
            last_seen_status,
            &utcnow_iso(),
        )
    }

    /// Upsert offer state at an explicit timestamp.
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

    /// List a page of open (or pending visibility) offer states.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_open_offer_states_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> SignerResult<Vec<OfferStateListRow>> {
        let Some(limit_i64) = limit_i64(limit, "list_open_offer_states_page")? else {
            return Ok(Vec::new());
        };
        let offset_i64 = i64::try_from(offset).map_err(|_| {
            SignerError::Other("list_open_offer_states_page offset exceeds i64 max".to_string())
        })?;
        let open = ReconcileState::Lifecycle(OfferLifecycleState::Open);
        let pending = ReconcileState::PendingVisibility;
        query_mapped(
            &self.conn,
            &format!(
                r"
                SELECT {OFFER_STATE_LIST_COLUMNS}
                FROM offer_state
                WHERE state IN (?1, ?2)
                ORDER BY offer_id ASC
                LIMIT ?3 OFFSET ?4
                "
            ),
            params![open.as_str(), pending.as_str(), limit_i64, offset_i64],
            "open offer_state",
            read_offer_state_list_row,
        )
    }

    /// Ladder-capacity candidate rows for a market (paginated; no silent truncation).
    ///
    /// Includes `open` / `refresh_due` / `maker_claimed` and `mempool_observed` (caller
    /// applies the recent-age gate for mempool). Newest-first order is not required —
    /// capacity counting is a full set.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_ladder_capacity_offer_states(
        &self,
        market_id: &str,
    ) -> SignerResult<Vec<OfferStateListRow>> {
        const PAGE_SIZE: i64 = 500;
        let market = market_id.trim();
        if market.is_empty() {
            return Ok(Vec::new());
        }
        let mut all = Vec::new();
        let mut offset: i64 = 0;
        loop {
            let page = self.list_ladder_capacity_offer_states_page(market, PAGE_SIZE, offset)?;
            let page_len = i64::try_from(page.len()).unwrap_or(i64::MAX);
            all.extend(page);
            if page_len < PAGE_SIZE {
                break;
            }
            offset = offset.saturating_add(PAGE_SIZE);
        }
        Ok(all)
    }

    fn list_ladder_capacity_offer_states_page(
        &self,
        market_id: &str,
        limit: i64,
        offset: i64,
    ) -> SignerResult<Vec<OfferStateListRow>> {
        let state_placeholders = (1..=LADDER_CAPACITY_QUERY_STATES.len())
            .map(|idx| format!("?{idx}"))
            .collect::<Vec<_>>()
            .join(", ");
        let market_idx = LADDER_CAPACITY_QUERY_STATES.len() + 1;
        let limit_idx = market_idx + 1;
        let offset_idx = limit_idx + 1;
        let sql = format!(
            r"
            SELECT {OFFER_STATE_LIST_COLUMNS}
            FROM offer_state
            WHERE state IN ({state_placeholders})
              AND market_id = ?{market_idx}
            ORDER BY offer_id ASC
            LIMIT ?{limit_idx} OFFSET ?{offset_idx}
            "
        );
        let mut values: Vec<rusqlite::types::Value> = LADDER_CAPACITY_QUERY_STATES
            .iter()
            .map(|state| (*state).to_string().into())
            .collect();
        values.push(market_id.to_string().into());
        values.push(limit.into());
        values.push(offset.into());
        query_mapped(
            &self.conn,
            &sql,
            rusqlite::params_from_iter(values),
            "ladder capacity offer_state",
            read_offer_state_list_row,
        )
    }

    /// List all open (or pending visibility) offer states without recency bias.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_all_open_offer_states(&self) -> SignerResult<Vec<OfferStateListRow>> {
        const PAGE_SIZE: usize = 1_000;
        let mut all = Vec::new();
        let mut offset = 0_usize;
        loop {
            let page = self.list_open_offer_states_page(PAGE_SIZE, offset)?;
            let count = page.len();
            all.extend(page);
            if count < PAGE_SIZE {
                break;
            }
            offset = offset.saturating_add(PAGE_SIZE);
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
        let rows = self.query_offer_state_list_by_ids(
            &clean_ids,
            "offer_id IN ({in})",
            "offer_state by ids",
        )?;
        let by_id: HashMap<_, _> = rows
            .into_iter()
            .map(|row| (row.offer_id.clone(), row))
            .collect();
        Ok(clean_ids
            .into_iter()
            .filter_map(|offer_id| by_id.get(&offer_id).cloned())
            .collect())
    }

    /// Distinct non-empty `cancel_submitted_tx_id` values for in-flight cancels.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_cancel_submitted_tx_ids(&self) -> SignerResult<Vec<String>> {
        let rows: Vec<String> = query_mapped(
            &self.conn,
            r"
            SELECT DISTINCT cancel_submitted_tx_id
            FROM offer_state
            WHERE state = 'cancel_submitted'
              AND cancel_submitted_tx_id IS NOT NULL
              AND length(trim(cancel_submitted_tx_id)) > 0
            ",
            [],
            "cancel_submitted tx list",
            |row| row.get(0),
        )?;
        let mut out: Vec<String> = rows
            .into_iter()
            .filter_map(|raw| canonical_tx_id(&raw))
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// List `cancel_submitted` offers whose cancel tx id is in `tx_ids`.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_offer_states_for_cancel_submitted_tx_ids(
        &self,
        tx_ids: &[String],
    ) -> SignerResult<Vec<OfferStateListRow>> {
        let clean_ids: Vec<String> = tx_ids
            .iter()
            .filter_map(|value| canonical_tx_id(value))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        self.query_offer_state_list_by_ids(
            &clean_ids,
            "state = 'cancel_submitted' AND cancel_submitted_tx_id IN ({in})",
            "cancel_submitted by tx ids",
        )
    }

    /// List offer states, optionally filtered by market, newest first.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_offer_states(
        &self,
        market_id: Option<&str>,
        limit: usize,
    ) -> SignerResult<Vec<OfferStateListRow>> {
        let Some(limit_i64) = limit_i64(limit, "list_offer_states")? else {
            return Ok(Vec::new());
        };
        let market = market_id.map(str::trim).filter(|value| !value.is_empty());
        query_mapped(
            &self.conn,
            &format!(
                r"
                SELECT {OFFER_STATE_LIST_COLUMNS}
                FROM offer_state
                WHERE ?1 IS NULL OR market_id = ?1
                ORDER BY updated_at DESC
                LIMIT ?2
                "
            ),
            params![market, limit_i64],
            "offer_state list",
            read_offer_state_list_row,
        )
    }

    /// Current lifecycle state for one offer id, if present.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) fn offer_state_for_id(&self, offer_id: &str) -> SignerResult<Option<String>> {
        self.conn
            .query_row(
                "SELECT state FROM offer_state WHERE offer_id = ?1",
                params![offer_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| db_err("offer_state by id", err))
    }

    /// Maker coin ids still bound to an open/in-flight offer for `market_id`.
    ///
    /// Uses `BINDING_MAKER_QUERY_STATES` / `ReconcileState::binds_unique_maker_coin`.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_binding_maker_coin_ids(&self, market_id: &str) -> SignerResult<Vec<String>> {
        let market = market_id.trim();
        if market.is_empty() {
            return Ok(Vec::new());
        }
        let state_placeholders = in_placeholders(BINDING_MAKER_QUERY_STATES.len());
        let market_idx = BINDING_MAKER_QUERY_STATES.len() + 1;
        let sql = format!(
            r"
            SELECT cancel_input_coin_id
            FROM offer_state
            WHERE state IN ({state_placeholders})
              AND market_id = ?{market_idx}
              AND cancel_input_coin_id IS NOT NULL
              AND TRIM(cancel_input_coin_id) != ''
            "
        );
        let mut values: Vec<rusqlite::types::Value> = BINDING_MAKER_QUERY_STATES
            .iter()
            .map(|state| (*state).to_string().into())
            .collect();
        values.push(market.to_string().into());
        let rows: Vec<String> = query_mapped(
            &self.conn,
            &sql,
            rusqlite::params_from_iter(values),
            "binding maker coin ids",
            |row| row.get(0),
        )?;
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for raw_coin_id in rows {
            let coin_id = normalize_hex_id(&raw_coin_id);
            if coin_id.is_empty() || !seen.insert(coin_id.clone()) {
                continue;
            }
            out.push(coin_id);
        }
        out.sort();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
    use crate::storage::sqlite::offer_cancel::{OfferCancelWrite, OfferListingWrite};
    use tempfile::tempdir;

    fn insert_maker(
        store: &SqliteStore,
        offer_id: &str,
        state: &str,
        coin_byte: u8,
        dexie_status: Option<i64>,
    ) {
        let coin = hex::encode([coin_byte; 32]);
        let fields = OfferCancelFields::from_presplit_build(coin, "bb".repeat(32), "cc".repeat(32));
        store
            .upsert_offer_state_with_metadata_at(
                offer_id,
                "m1",
                state,
                dexie_status,
                "2026-01-01T00:00:00Z",
                OfferCancelWrite {
                    fields: Some(&fields),
                    execution_mode: Some(OfferExecutionMode::Direct),
                    listing: OfferListingWrite {
                        publish_venue: Some("dexie"),
                        listing_expires_at: None,
                        size_base_units: Some(10),
                        offer_nonce: None,
                        offer_side: Some("sell"),
                    },
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert");
    }

    #[test]
    fn list_binding_maker_coin_ids_includes_open_and_pending_excludes_expired() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        insert_maker(&store, "o-open", "open", 0xaa, None);
        insert_maker(&store, "o-pending", "pending_visibility", 0xbb, None);
        insert_maker(&store, "o-expired", "expired", 0xcc, Some(6));
        insert_maker(&store, "o-claimed", "maker_claimed", 0xdd, None);

        let ids = store
            .list_binding_maker_coin_ids("m1")
            .expect("binding makers");
        assert_eq!(
            ids,
            vec![
                hex::encode([0xaa; 32]),
                hex::encode([0xbb; 32]),
                hex::encode([0xdd; 32]),
            ]
        );
    }
}
