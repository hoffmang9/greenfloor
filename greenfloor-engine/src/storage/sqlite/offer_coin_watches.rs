//! Durable offer coin / p2 watches for Coinset WS lifecycle matching.
//!
//! The `coin_id` column stores a watch key: either a maker coin id (`kind='coin'`)
//! or an on-chain maker puzzle hash (`kind='p2'`).

use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

use super::{utcnow_iso, SqliteStore};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::storage::OfferStateListRow;

/// How a durable watch row matched observed WS keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchMatchKind {
    Coin,
    P2,
    Both,
}

impl WatchMatchKind {
    #[must_use]
    pub const fn includes_coin(self) -> bool {
        matches!(self, Self::Coin | Self::Both)
    }

    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Both, _) | (_, Self::Both) | (Self::Coin, Self::P2) | (Self::P2, Self::Coin) => {
                Self::Both
            }
            (Self::Coin, Self::Coin) => Self::Coin,
            (Self::P2, Self::P2) => Self::P2,
        }
    }

    fn from_kind_str(kind: &str) -> Option<Self> {
        match kind {
            "coin" => Some(Self::Coin),
            "p2" => Some(Self::P2),
            _ => None,
        }
    }
}

/// Offer state row plus the watch kind(s) that matched the query keys.
#[derive(Debug, Clone)]
pub struct WatchHitRow {
    pub row: OfferStateListRow,
    pub kind: WatchMatchKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WatchInsertMode {
    /// DELETE existing rows for the offer, then INSERT. Dedup. Error if all keys invalid.
    Replace,
    /// INSERT OR IGNORE. Skip invalid keys silently.
    Ensure,
}

/// Insert coin/p2 watch rows. `coin_id` column holds the watch key (coin id or p2).
///
/// # Errors
///
/// Returns an error if `SQLite` writes fail, or (Replace) when all provided keys are invalid.
pub(crate) fn insert_watch_rows(
    conn: &Connection,
    offer_id: &str,
    market_id: &str,
    coin_ids: &[String],
    p2s: &[String],
    mode: WatchInsertMode,
) -> SignerResult<usize> {
    let clean_offer = offer_id.trim();
    let clean_market = market_id.trim();
    if clean_offer.is_empty() || clean_market.is_empty() {
        return Err(SignerError::Other(
            "offer_id and market_id are required for offer_coin_watches".to_string(),
        ));
    }
    let now = utcnow_iso();
    if mode == WatchInsertMode::Replace {
        conn.execute(
            "DELETE FROM offer_coin_watches WHERE offer_id = ?1",
            params![clean_offer],
        )
        .map_err(|err| SignerError::Other(format!("offer_coin_watches delete: {err}")))?;
    }

    let sql = match mode {
        WatchInsertMode::Replace => {
            r"
            INSERT INTO offer_coin_watches (coin_id, offer_id, market_id, kind, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "
        }
        WatchInsertMode::Ensure => {
            r"
            INSERT OR IGNORE INTO offer_coin_watches (coin_id, offer_id, market_id, kind, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "
        }
    };

    let mut seen = HashSet::new();
    let mut inserted = 0usize;
    for (kind, keys) in [("coin", coin_ids), ("p2", p2s)] {
        for key in keys {
            let normalized = normalize_hex_id(key);
            if normalized.len() != 64 {
                if mode == WatchInsertMode::Replace {
                    tracing::warn!(
                        offer_id = clean_offer,
                        market_id = clean_market,
                        kind,
                        raw_len = key.trim().len(),
                        normalized_len = normalized.len(),
                        "skipping non-64-char watch key for offer_coin_watches"
                    );
                }
                continue;
            }
            if mode == WatchInsertMode::Replace && !seen.insert(normalized.clone()) {
                continue;
            }
            conn.execute(
                sql,
                params![normalized, clean_offer, clean_market, kind, now],
            )
            .map_err(|err| {
                SignerError::Other(format!("offer_coin_watches insert {kind}: {err}"))
            })?;
            inserted += 1;
        }
    }
    if mode == WatchInsertMode::Replace
        && inserted == 0
        && (!coin_ids.is_empty() || !p2s.is_empty())
    {
        return Err(SignerError::Other(format!(
            "offer_coin_watches for offer {clean_offer}: all {coin_count} coin ids and {p2_count} p2s were invalid or empty after normalize",
            coin_count = coin_ids.len(),
            p2_count = p2s.len(),
        )));
    }
    Ok(inserted)
}

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
        self.unchecked_transaction_scope("offer_coin_watches", |store| {
            store.replace_offer_coin_watches_no_txn(offer_id, market_id, coin_ids, p2s)
        })
    }

    /// Insert missing coin/p2 watches without clearing existing rows (`INSERT OR IGNORE`).
    ///
    /// Used to heal pre-upgrade Dexie offers that never received watch backfill.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` writes fail.
    pub fn ensure_offer_coin_watches(
        &self,
        offer_id: &str,
        market_id: &str,
        coin_ids: &[String],
        p2s: &[String],
    ) -> SignerResult<()> {
        insert_watch_rows(
            &self.conn,
            offer_id,
            market_id,
            coin_ids,
            p2s,
            WatchInsertMode::Ensure,
        )?;
        Ok(())
    }

    /// True when the offer has at least one durable coin or p2 watch.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn offer_has_coin_watches(&self, offer_id: &str) -> SignerResult<bool> {
        let clean = offer_id.trim();
        if clean.is_empty() {
            return Ok(false);
        }
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM offer_coin_watches WHERE offer_id = ?1",
                params![clean],
                |row| row.get(0),
            )
            .map_err(|err| SignerError::Other(format!("offer_coin_watches count: {err}")))?;
        Ok(count > 0)
    }

    /// Replace watches without opening a transaction (caller must hold one).
    pub(crate) fn replace_offer_coin_watches_no_txn(
        &self,
        offer_id: &str,
        market_id: &str,
        coin_ids: &[String],
        p2s: &[String],
    ) -> SignerResult<()> {
        insert_watch_rows(
            &self.conn,
            offer_id,
            market_id,
            coin_ids,
            p2s,
            WatchInsertMode::Replace,
        )?;
        Ok(())
    }

    /// List coin and p2 watches for one offer (post-time set for cancel rollback).
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_offer_coin_watches_for_offer(
        &self,
        offer_id: &str,
    ) -> SignerResult<(Vec<String>, Vec<String>)> {
        let clean_offer = offer_id.trim();
        if clean_offer.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT coin_id, kind FROM offer_coin_watches WHERE offer_id = ?1 ORDER BY kind, coin_id",
            )
            .map_err(|err| SignerError::Other(format!("offer_coin_watches list prepare: {err}")))?;
        let rows = stmt
            .query_map(params![clean_offer], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|err| SignerError::Other(format!("offer_coin_watches list query: {err}")))?;
        let mut coins = Vec::new();
        let mut p2s = Vec::new();
        for row in rows {
            let (raw_id, kind) = row
                .map_err(|err| SignerError::Other(format!("offer_coin_watches list row: {err}")))?;
            let normalized = normalize_hex_id(&raw_id);
            if normalized.len() != 64 {
                continue;
            }
            match kind.as_str() {
                "coin" => coins.push(normalized),
                "p2" => p2s.push(normalized),
                other => {
                    tracing::warn!(
                        offer_id = clean_offer,
                        kind = other,
                        "skipping unknown offer_coin_watches kind"
                    );
                }
            }
        }
        Ok((coins, p2s))
    }

    /// Clear watches for one offer (terminal lifecycle persist only).
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

    /// List distinct watched maker coin ids for a market (`kind = 'coin'` only).
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_watched_coin_ids_for_market(
        &self,
        market_id: &str,
    ) -> SignerResult<HashSet<String>> {
        self.list_watched_keys_for_market(market_id, "coin")
    }

    /// List distinct watched maker p2 hashes for a market (`kind = 'p2'` only).
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_watched_p2s_for_market(&self, market_id: &str) -> SignerResult<HashSet<String>> {
        self.list_watched_keys_for_market(market_id, "p2")
    }

    fn list_watched_keys_for_market(
        &self,
        market_id: &str,
        kind: &str,
    ) -> SignerResult<HashSet<String>> {
        let clean_market = market_id.trim();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT coin_id FROM offer_coin_watches WHERE market_id = ?1 AND kind = ?2",
            )
            .map_err(|err| SignerError::Other(format!("offer_coin_watches prepare: {err}")))?;
        let rows = stmt
            .query_map(params![clean_market, kind], |row| row.get::<_, String>(0))
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

    /// List offer state rows watching any of the given coin/p2 keys (deduped by `offer_id`).
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_offer_states_for_watched_keys(
        &self,
        keys: &[String],
    ) -> SignerResult<Vec<OfferStateListRow>> {
        Ok(self
            .list_offer_states_for_watched_keys_with_kind(keys)?
            .into_iter()
            .map(|hit| hit.row)
            .collect())
    }

    /// List offer state rows with the watch kind(s) that matched `keys`.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_offer_states_for_watched_keys_with_kind(
        &self,
        keys: &[String],
    ) -> SignerResult<Vec<WatchHitRow>> {
        let kind_by_offer = self.query_offer_watch_kinds(keys)?;
        if kind_by_offer.is_empty() {
            return Ok(Vec::new());
        }
        let offer_ids: Vec<String> = kind_by_offer.keys().cloned().collect();
        let rows = self.list_offer_states_for_ids(&offer_ids)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                kind_by_offer
                    .get(&row.offer_id)
                    .copied()
                    .map(|kind| WatchHitRow { row, kind })
            })
            .collect())
    }

    fn query_offer_watch_kinds(
        &self,
        keys: &[String],
    ) -> SignerResult<HashMap<String, WatchMatchKind>> {
        let normalized: Vec<String> = keys
            .iter()
            .map(|key| normalize_hex_id(key))
            .filter(|key| key.len() == 64)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if normalized.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders: Vec<String> = (1..=normalized.len())
            .map(|idx| format!("?{idx}"))
            .collect();
        let query = format!(
            "SELECT DISTINCT offer_id, kind FROM offer_coin_watches WHERE coin_id IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|err| SignerError::Other(format!("offer_coin_watches kind prepare: {err}")))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(normalized.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|err| SignerError::Other(format!("offer_coin_watches kind query: {err}")))?;
        let mut kind_by_offer: HashMap<String, WatchMatchKind> = HashMap::new();
        for row in rows {
            let (offer_id, kind_str) = row
                .map_err(|err| SignerError::Other(format!("offer_coin_watches kind row: {err}")))?;
            let Some(kind) = WatchMatchKind::from_kind_str(kind_str.as_str()) else {
                tracing::warn!(
                    offer_id = %offer_id,
                    kind = %kind_str,
                    "skipping unknown offer_coin_watches kind in match query"
                );
                continue;
            };
            kind_by_offer
                .entry(offer_id)
                .and_modify(|existing| *existing = existing.merge(kind))
                .or_insert(kind);
        }
        Ok(kind_by_offer)
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
        let watched_p2s = store.list_watched_p2s_for_market("m1").expect("p2s");
        assert!(watched_p2s.contains(&p2));
        assert!(!watched_p2s.contains(&coin));
        let offers = store
            .list_offer_ids_for_watched_coin(&coin)
            .expect("offers");
        assert_eq!(offers, vec!["offer1".to_string()]);
        let (coins, p2s) = store
            .list_offer_coin_watches_for_offer("offer1")
            .expect("by offer");
        assert_eq!(coins, vec![coin.clone()]);
        assert_eq!(p2s, vec![p2]);
        store.clear_offer_coin_watches("offer1").expect("clear");
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("list")
            .is_empty());
        assert!(store
            .list_watched_p2s_for_market("m1")
            .expect("p2s")
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
    fn list_offer_states_for_watched_keys_with_kind_aggregates() {
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
        let hits = store
            .list_offer_states_for_watched_keys_with_kind(&[coin, p2])
            .expect("hits");
        assert_eq!(hits.len(), 2);
        let mut by_id: HashMap<_, _> = hits
            .into_iter()
            .map(|hit| (hit.row.offer_id, (hit.row.state, hit.kind)))
            .collect();
        assert_eq!(
            by_id.remove(&offer_a),
            Some(("open".to_string(), WatchMatchKind::Both))
        );
        assert_eq!(
            by_id.remove(&offer_b),
            Some(("mempool_observed".to_string(), WatchMatchKind::Coin))
        );
    }
}
