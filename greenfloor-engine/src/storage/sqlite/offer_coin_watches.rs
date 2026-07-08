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
        for coin_id in coin_ids {
            let normalized = normalize_hex_id(coin_id);
            if normalized.len() != 64 || !seen.insert(normalized.clone()) {
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
        }
        for p2 in p2s {
            let normalized = normalize_hex_id(p2);
            if normalized.len() != 64 || !seen.insert(normalized.clone()) {
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
            "SELECT DISTINCT market_id FROM offer_coin_watches WHERE coin_id IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = self.conn.prepare(&query).map_err(|err| {
            SignerError::Other(format!("offer_coin_watches market prepare: {err}"))
        })?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(normalized.iter()), |row| {
                row.get::<_, String>(0)
            })
            .map_err(|err| SignerError::Other(format!("offer_coin_watches market query: {err}")))?;
        let mut out = Vec::new();
        for row in rows {
            let market_id = row.map_err(|err| {
                SignerError::Other(format!("offer_coin_watches market row: {err}"))
            })?;
            if !market_id.trim().is_empty() {
                out.push(market_id);
            }
        }
        out.sort();
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
        let offer_ids: Vec<String> = {
            let placeholders: Vec<String> = (1..=normalized.len())
                .map(|idx| format!("?{idx}"))
                .collect();
            let query = format!(
                "SELECT DISTINCT offer_id FROM offer_coin_watches WHERE coin_id IN ({})",
                placeholders.join(", ")
            );
            let mut stmt = self.conn.prepare(&query).map_err(|err| {
                SignerError::Other(format!("offer_coin_watches offer_ids prepare: {err}"))
            })?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(normalized.iter()), |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|err| {
                    SignerError::Other(format!("offer_coin_watches offer_ids query: {err}"))
                })?;
            let mut out = Vec::new();
            for row in rows {
                let offer_id = row.map_err(|err| {
                    SignerError::Other(format!("offer_coin_watches offer_ids row: {err}"))
                })?;
                if !offer_id.trim().is_empty() {
                    out.push(offer_id);
                }
            }
            out
        };
        self.list_offer_states_for_ids(&offer_ids)
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
}
