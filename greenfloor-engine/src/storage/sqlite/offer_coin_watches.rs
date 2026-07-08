//! Durable offer coin / p2 watches for Coinset WS lifecycle matching.

use rusqlite::params;
use std::collections::HashSet;

use super::{utcnow_iso, SqliteStore};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;

impl SqliteStore {
    /// Replace all watches for one offer with the provided coin ids / p2s.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` writes fail.
    pub fn replace_offer_coin_watches(
        &self,
        offer_id: &str,
        market_id: &str,
        coin_ids: &[String],
        p2s: &[String],
    ) -> SignerResult<()> {
        let clean_offer = offer_id.trim();
        let clean_market = market_id.trim();
        if clean_offer.is_empty() || clean_market.is_empty() {
            return Err(SignerError::Other(
                "offer_id and market_id are required for offer_coin_watches".to_string(),
            ));
        }
        let now = utcnow_iso();
        let tx = self.conn.unchecked_transaction().map_err(|err| {
            SignerError::Other(format!("offer_coin_watches begin transaction: {err}"))
        })?;
        tx.execute(
            "DELETE FROM offer_coin_watches WHERE offer_id = ?1",
            params![clean_offer],
        )
        .map_err(|err| SignerError::Other(format!("offer_coin_watches delete: {err}")))?;

        let mut seen = HashSet::new();
        let mut inserted = 0usize;
        for coin_id in coin_ids {
            let normalized = normalize_hex_id(coin_id);
            if normalized.len() != 64 {
                tracing::warn!(
                    offer_id = clean_offer,
                    market_id = clean_market,
                    kind = "coin",
                    raw_len = coin_id.trim().len(),
                    normalized_len = normalized.len(),
                    "skipping non-64-char coin id for offer_coin_watches"
                );
                continue;
            }
            if !seen.insert(normalized.clone()) {
                continue;
            }
            tx.execute(
                r"
                INSERT INTO offer_coin_watches (coin_id, offer_id, market_id, kind, updated_at)
                VALUES (?1, ?2, ?3, 'coin', ?4)
                ",
                params![normalized, clean_offer, clean_market, now],
            )
            .map_err(|err| SignerError::Other(format!("offer_coin_watches insert coin: {err}")))?;
            inserted += 1;
        }
        for p2 in p2s {
            let normalized = normalize_hex_id(p2);
            if normalized.len() != 64 {
                tracing::warn!(
                    offer_id = clean_offer,
                    market_id = clean_market,
                    kind = "p2",
                    raw_len = p2.trim().len(),
                    normalized_len = normalized.len(),
                    "skipping non-64-char p2 for offer_coin_watches"
                );
                continue;
            }
            if !seen.insert(normalized.clone()) {
                continue;
            }
            tx.execute(
                r"
                INSERT INTO offer_coin_watches (coin_id, offer_id, market_id, kind, updated_at)
                VALUES (?1, ?2, ?3, 'p2', ?4)
                ",
                params![normalized, clean_offer, clean_market, now],
            )
            .map_err(|err| SignerError::Other(format!("offer_coin_watches insert p2: {err}")))?;
            inserted += 1;
        }
        if inserted == 0 && (!coin_ids.is_empty() || !p2s.is_empty()) {
            return Err(SignerError::Other(format!(
                "offer_coin_watches for offer {clean_offer}: all {coin_count} coin ids and {p2_count} p2s were invalid or empty after normalize",
                coin_count = coin_ids.len(),
                p2_count = p2s.len(),
            )));
        }
        tx.commit()
            .map_err(|err| SignerError::Other(format!("offer_coin_watches commit: {err}")))?;
        Ok(())
    }

    /// Clear watches for one offer (terminal lifecycle).
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` writes fail.
    pub fn clear_offer_coin_watches(&self, offer_id: &str) -> SignerResult<()> {
        let clean_offer = offer_id.trim();
        if clean_offer.is_empty() {
            return Ok(());
        }
        self.conn
            .execute(
                "DELETE FROM offer_coin_watches WHERE offer_id = ?1",
                params![clean_offer],
            )
            .map_err(|err| SignerError::Other(format!("offer_coin_watches clear: {err}")))?;
        Ok(())
    }

    /// List distinct watched coin/p2 ids for a market.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_watched_coin_ids_for_market(
        &self,
        market_id: &str,
    ) -> SignerResult<HashSet<String>> {
        let clean_market = market_id.trim();
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT coin_id FROM offer_coin_watches WHERE market_id = ?1 AND kind = 'coin'")
            .map_err(|err| SignerError::Other(format!("offer_coin_watches prepare: {err}")))?;
        let rows = stmt
            .query_map(params![clean_market], |row| row.get::<_, String>(0))
            .map_err(|err| SignerError::Other(format!("offer_coin_watches query: {err}")))?;
        let mut out = HashSet::default();
        for row in rows {
            let value =
                row.map_err(|err| SignerError::Other(format!("offer_coin_watches row: {err}")))?;
            let normalized = normalize_hex_id(&value);
            if normalized.len() == 64 {
                out.insert(normalized);
            }
        }
        Ok(out)
    }

    /// List distinct market ids watching any of the given coin/p2 keys.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_market_ids_for_watched_keys(&self, keys: &[String]) -> SignerResult<Vec<String>> {
        let mut markets = self.query_distinct_watch_column("market_id", keys)?;
        markets.sort();
        Ok(markets)
    }

    /// Sync durable watches for watched lifecycle offers in one market pass.
    ///
    /// Upgrade/migration helper: seed missing watches from cancel/presplit metadata.
    /// Prefer atomic post persist + schema migration backfill for production paths.
    ///
    /// Market inventory receive/CAT outer puzzle hashes are intentionally **not**
    /// written into per-offer watches.
    ///
    /// Idempotent. Returns the number of offers whose watch rows were written.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads or writes fail.
    pub fn sync_offer_watches_for_market(&self, market_id: &str) -> SignerResult<u64> {
        let clean_market = market_id.trim();
        if clean_market.is_empty() {
            return Ok(0);
        }
        let rows = self.list_offer_states(Some(clean_market), 5000)?;
        let mut updated = 0u64;
        for row in rows {
            let Ok(state) = crate::cycle::ReconcileState::parse(&row.state) else {
                continue;
            };
            if !state.is_watched_for_reconcile() {
                continue;
            }
            let existing_coins = self.list_watched_coin_ids_for_offer(&row.offer_id)?;
            let existing_p2s = self.list_watched_p2s_for_offer(&row.offer_id)?;
            if !existing_coins.is_empty() || !existing_p2s.is_empty() {
                continue;
            }
            let Some(meta) = self.offer_cancel_metadata_for_id(&row.offer_id)? else {
                continue;
            };
            let mut coins = Vec::new();
            if let Some(coin) = meta.fields.input_coin_id {
                coins.push(coin);
            }
            let mut p2s = Vec::new();
            if let Some(p2) = meta.fields.fixed_delegated_puzzle_hash {
                p2s.push(normalize_hex_id(&p2));
            }
            if coins.is_empty() && p2s.is_empty() {
                continue;
            }
            self.replace_offer_coin_watches(&row.offer_id, &row.market_id, &coins, &p2s)?;
            updated += 1;
        }
        Ok(updated)
    }

    fn list_watched_coin_ids_for_offer(&self, offer_id: &str) -> SignerResult<Vec<String>> {
        let clean = offer_id.trim();
        if clean.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare("SELECT coin_id FROM offer_coin_watches WHERE offer_id = ?1 AND kind = 'coin'")
            .map_err(|err| SignerError::Other(format!("offer_coin_watches offer coins: {err}")))?;
        let rows = stmt
            .query_map(params![clean], |row| row.get::<_, String>(0))
            .map_err(|err| {
                SignerError::Other(format!("offer_coin_watches offer coins query: {err}"))
            })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| {
                SignerError::Other(format!("offer_coin_watches offer coins row: {err}"))
            })?);
        }
        Ok(out)
    }

    fn list_watched_p2s_for_offer(&self, offer_id: &str) -> SignerResult<Vec<String>> {
        let clean = offer_id.trim();
        if clean.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare("SELECT coin_id FROM offer_coin_watches WHERE offer_id = ?1 AND kind = 'p2'")
            .map_err(|err| SignerError::Other(format!("offer_coin_watches offer p2s: {err}")))?;
        let rows = stmt
            .query_map(params![clean], |row| row.get::<_, String>(0))
            .map_err(|err| {
                SignerError::Other(format!("offer_coin_watches offer p2s query: {err}"))
            })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| {
                SignerError::Other(format!("offer_coin_watches offer p2s row: {err}"))
            })?);
        }
        Ok(out)
    }

    /// List offer state rows watching any of the given coin/p2 keys (deduped by `offer_id`).
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_offer_states_for_watched_keys(
        &self,
        keys: &[String],
    ) -> SignerResult<Vec<crate::storage::OfferStateListRow>> {
        let offer_ids = self.query_distinct_watch_column("offer_id", keys)?;
        self.list_offer_states_for_ids(&offer_ids)
    }

    fn query_distinct_watch_column(
        &self,
        column: &str,
        keys: &[String],
    ) -> SignerResult<Vec<String>> {
        let normalized: Vec<String> = keys
            .iter()
            .map(|key| normalize_hex_id(key))
            .filter(|key| key.len() == 64)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if normalized.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: Vec<String> = (1..=normalized.len())
            .map(|idx| format!("?{idx}"))
            .collect();
        let query = format!(
            "SELECT DISTINCT {column} FROM offer_coin_watches WHERE coin_id IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = self.conn.prepare(&query).map_err(|err| {
            SignerError::Other(format!("offer_coin_watches {column} prepare: {err}"))
        })?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(normalized.iter()), |row| {
                row.get::<_, String>(0)
            })
            .map_err(|err| {
                SignerError::Other(format!("offer_coin_watches {column} query: {err}"))
            })?;
        let mut out = Vec::new();
        for row in rows {
            let value = row.map_err(|err| {
                SignerError::Other(format!("offer_coin_watches {column} row: {err}"))
            })?;
            if !value.trim().is_empty() {
                out.push(value);
            }
        }
        Ok(out)
    }

    /// List offer ids watching a given coin id or p2.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_offer_ids_for_watched_coin(&self, coin_id: &str) -> SignerResult<Vec<String>> {
        let normalized = normalize_hex_id(coin_id);
        if normalized.len() != 64 {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT offer_id FROM offer_coin_watches WHERE coin_id = ?1")
            .map_err(|err| SignerError::Other(format!("offer_coin_watches prepare: {err}")))?;
        let rows = stmt
            .query_map(params![normalized], |row| row.get::<_, String>(0))
            .map_err(|err| SignerError::Other(format!("offer_coin_watches query: {err}")))?;
        let mut out = Vec::new();
        for row in rows {
            let offer_id =
                row.map_err(|err| SignerError::Other(format!("offer_coin_watches row: {err}")))?;
            if !offer_id.trim().is_empty() {
                out.push(offer_id);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn replace_and_list_offer_coin_watches() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let coin = "ab".repeat(32);
        let p2 = "cd".repeat(32);
        store
            .replace_offer_coin_watches(
                "offer1",
                "m1",
                std::slice::from_ref(&coin),
                std::slice::from_ref(&p2),
            )
            .expect("replace");
        let watched = store.list_watched_coin_ids_for_market("m1").expect("list");
        assert!(watched.contains(&coin));
        assert!(!watched.contains(&p2));
        let offers = store
            .list_offer_ids_for_watched_coin(&coin)
            .expect("offers");
        assert_eq!(offers, vec!["offer1".to_string()]);
        store.clear_offer_coin_watches("offer1").expect("clear");
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("list")
            .is_empty());
    }

    #[test]
    fn list_market_ids_for_watched_keys() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let p2 = "cd".repeat(32);
        store
            .replace_offer_coin_watches("offer1", "m1", &[], std::slice::from_ref(&p2))
            .expect("replace");
        let markets = store
            .list_market_ids_for_watched_keys(&[p2])
            .expect("markets");
        assert_eq!(markets, vec!["m1".to_string()]);
    }

    #[test]
    fn replace_rejects_when_all_watch_keys_invalid() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let err = store
            .replace_offer_coin_watches(
                "offer1",
                "m1",
                &["short".to_string()],
                &["also-bad".to_string()],
            )
            .expect_err("invalid keys");
        assert!(err.to_string().contains("all"), "unexpected error: {err}");
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("list")
            .is_empty());
    }

    #[test]
    fn replace_with_empty_inputs_clears_watches() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let coin = "ab".repeat(32);
        store
            .replace_offer_coin_watches("offer1", "m1", std::slice::from_ref(&coin), &[])
            .expect("seed");
        store
            .replace_offer_coin_watches("offer1", "m1", &[], &[])
            .expect("clear via empty replace");
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("list")
            .is_empty());
    }

    #[test]
    fn list_offer_states_for_watched_keys_dedupes_and_joins_state() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let offer_a = "aa".repeat(32);
        let offer_b = "bb".repeat(32);
        let coin = "11".repeat(32);
        let p2 = "22".repeat(32);
        store
            .upsert_offer_state(&offer_a, "m1", "open", None)
            .expect("a");
        store
            .upsert_offer_state(&offer_b, "m1", "mempool_observed", None)
            .expect("b");
        store
            .replace_offer_coin_watches(
                &offer_a,
                "m1",
                std::slice::from_ref(&coin),
                std::slice::from_ref(&p2),
            )
            .expect("watch a");
        store
            .replace_offer_coin_watches(&offer_b, "m1", std::slice::from_ref(&coin), &[])
            .expect("watch b");
        let rows = store
            .list_offer_states_for_watched_keys(&[coin, p2])
            .expect("rows");
        assert_eq!(rows.len(), 2);
        let mut by_id: std::collections::HashMap<_, _> = rows
            .into_iter()
            .map(|row| (row.offer_id, row.state))
            .collect();
        assert_eq!(by_id.remove(&offer_a).as_deref(), Some("open"));
        assert_eq!(by_id.remove(&offer_b).as_deref(), Some("mempool_observed"));
    }

    #[test]
    fn sync_offer_watches_seeds_from_cancel_metadata_only() {
        use crate::offer::types::{OfferExecutionMode, PresplitCancelFields};
        use crate::storage::sqlite::OfferCancelWrite;

        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let offer_id = "ab".repeat(32);
        let coin = "cd".repeat(32);
        let meta_p2 = "ef".repeat(32);
        let fields = PresplitCancelFields {
            input_coin_id: Some(coin.clone()),
            fixed_delegated_puzzle_hash: Some(meta_p2.clone()),
        };
        store
            .upsert_offer_state_with_metadata_at(
                &offer_id,
                "m1",
                "open",
                None,
                &super::utcnow_iso(),
                OfferCancelWrite {
                    fields: Some(&fields),
                    execution_mode: Some(OfferExecutionMode::PresplitExisting),
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert metadata");
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("empty")
            .is_empty());

        let updated = store.sync_offer_watches_for_market("m1").expect("sync");
        assert_eq!(updated, 1);
        let watched = store.list_watched_coin_ids_for_market("m1").expect("coins");
        assert!(watched.contains(&coin));
        assert!(!watched.contains(&meta_p2));
        assert_eq!(
            store
                .list_offer_ids_for_watched_coin(&meta_p2)
                .expect("meta p2"),
            vec![offer_id.clone()]
        );

        let updated_again = store
            .sync_offer_watches_for_market("m1")
            .expect("idempotent");
        assert_eq!(updated_again, 0);
    }

    #[test]
    fn sync_offer_watches_leaves_existing_coin_watches_unchanged() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let offer_id = "ab".repeat(32);
        let coin = "cd".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "open", None)
            .expect("upsert");
        store
            .replace_offer_coin_watches(&offer_id, "m1", std::slice::from_ref(&coin), &[])
            .expect("coin watch");
        let updated = store.sync_offer_watches_for_market("m1").expect("sync");
        assert_eq!(updated, 0);
        assert_eq!(
            store.list_offer_ids_for_watched_coin(&coin).expect("coin"),
            vec![offer_id]
        );
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("coins")
            .contains(&coin));
    }
}
