//! Durable offer watches for Coinset WS matching.
//!
//! Coin matches drive offer lifecycle. P2 matches mark inventory stale and exclude
//! watched spendable coins, but do not drive lifecycle.

mod types;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};

use super::{db_err, query_mapped, utcnow_iso, SqliteStore};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;

use types::WatchKind;
pub use types::{WatchHitRow, WatchMatchKind};

const INSERT_SQL: &str = "\
INSERT INTO offer_coin_watches (coin_id, offer_id, market_id, kind, updated_at) \
VALUES (?1, ?2, ?3, ?4, ?5)";

const INSERT_OR_IGNORE_SQL: &str = "\
INSERT OR IGNORE INTO offer_coin_watches (coin_id, offer_id, market_id, kind, updated_at) \
VALUES (?1, ?2, ?3, ?4, ?5)";

fn watch_key(raw: &str) -> Option<String> {
    let normalized = normalize_hex_id(raw);
    (normalized.len() == 64).then_some(normalized)
}

fn unique_watch_keys(keys: &[String]) -> Vec<String> {
    keys.iter()
        .filter_map(|key| watch_key(key))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

fn require_offer_market<'a>(
    offer_id: &'a str,
    market_id: &'a str,
) -> SignerResult<(&'a str, &'a str)> {
    let clean_offer = offer_id.trim();
    let clean_market = market_id.trim();
    if clean_offer.is_empty() || clean_market.is_empty() {
        return Err(SignerError::Other(
            "offer_id and market_id are required for offer_coin_watches".to_string(),
        ));
    }
    Ok((clean_offer, clean_market))
}

fn collect_watch_entries(coin_ids: &[String], p2s: &[String]) -> Vec<(String, WatchKind)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (kind, keys) in [(WatchKind::Coin, coin_ids), (WatchKind::P2, p2s)] {
        for key in keys {
            let Some(normalized) = watch_key(key) else {
                continue;
            };
            if seen.insert((normalized.clone(), kind)) {
                out.push((normalized, kind));
            }
        }
    }
    out
}

fn warn_invalid_watch_keys(offer_id: &str, market_id: &str, coin_ids: &[String], p2s: &[String]) {
    for (kind, keys) in [(WatchKind::Coin, coin_ids), (WatchKind::P2, p2s)] {
        for key in keys {
            let normalized = normalize_hex_id(key);
            if normalized.len() == 64 {
                continue;
            }
            tracing::warn!(
                offer_id,
                market_id,
                kind = kind.as_str(),
                raw_len = key.trim().len(),
                normalized_len = normalized.len(),
                "skipping non-64-char watch key for offer_coin_watches"
            );
        }
    }
}

fn insert_entries(
    conn: &Connection,
    offer_id: &str,
    market_id: &str,
    entries: &[(String, WatchKind)],
    sql: &'static str,
) -> SignerResult<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut stmt = conn
        .prepare(sql)
        .map_err(|err| db_err("offer_coin_watches insert prepare", err))?;
    let now = utcnow_iso();
    for (key, kind) in entries {
        stmt.execute(params![key, offer_id, market_id, kind.as_str(), now])
            .map_err(|err| db_err(&format!("offer_coin_watches insert {}", kind.as_str()), err))?;
    }
    Ok(())
}

fn in_placeholders(count: usize) -> String {
    (1..=count)
        .map(|idx| format!("?{idx}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// DELETE existing rows for the offer, then INSERT. Dedup. Error if all keys invalid.
pub(crate) fn replace_watch_rows(
    conn: &Connection,
    offer_id: &str,
    market_id: &str,
    coin_ids: &[String],
    p2s: &[String],
) -> SignerResult<()> {
    let (clean_offer, clean_market) = require_offer_market(offer_id, market_id)?;
    warn_invalid_watch_keys(clean_offer, clean_market, coin_ids, p2s);
    let entries = collect_watch_entries(coin_ids, p2s);
    conn.execute(
        "DELETE FROM offer_coin_watches WHERE offer_id = ?1",
        params![clean_offer],
    )
    .map_err(|err| db_err("offer_coin_watches delete", err))?;
    if entries.is_empty() {
        if coin_ids.is_empty() && p2s.is_empty() {
            return Ok(());
        }
        return Err(SignerError::Other(format!(
            "offer_coin_watches for offer {clean_offer}: all {coin_count} coin ids and {p2_count} p2s were invalid or empty after normalize",
            coin_count = coin_ids.len(),
            p2_count = p2s.len(),
        )));
    }
    insert_entries(conn, clean_offer, clean_market, &entries, INSERT_SQL)
}

/// INSERT OR IGNORE. Skip invalid keys silently.
pub(crate) fn ensure_watch_rows(
    conn: &Connection,
    offer_id: &str,
    market_id: &str,
    coin_ids: &[String],
    p2s: &[String],
) -> SignerResult<()> {
    let (clean_offer, clean_market) = require_offer_market(offer_id, market_id)?;
    let entries = collect_watch_entries(coin_ids, p2s);
    insert_entries(
        conn,
        clean_offer,
        clean_market,
        &entries,
        INSERT_OR_IGNORE_SQL,
    )
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
            replace_watch_rows(&store.conn, offer_id, market_id, coin_ids, p2s)
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
        ensure_watch_rows(&self.conn, offer_id, market_id, coin_ids, p2s)
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
        let exists: i64 = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM offer_coin_watches WHERE offer_id = ?1)",
                params![clean],
                |row| row.get(0),
            )
            .map_err(|err| db_err("offer_coin_watches exists", err))?;
        Ok(exists != 0)
    }

    /// Replace watches without opening a transaction (caller must hold one).
    pub(crate) fn replace_offer_coin_watches_no_txn(
        &self,
        offer_id: &str,
        market_id: &str,
        coin_ids: &[String],
        p2s: &[String],
    ) -> SignerResult<()> {
        replace_watch_rows(&self.conn, offer_id, market_id, coin_ids, p2s)
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
        let rows: Vec<(String, String)> = query_mapped(
            &self.conn,
            "SELECT coin_id, kind FROM offer_coin_watches WHERE offer_id = ?1 ORDER BY kind, coin_id",
            params![clean_offer],
            "offer_coin_watches list",
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut coins = Vec::new();
        let mut p2s = Vec::new();
        for (raw_id, kind_str) in rows {
            let Some(normalized) = watch_key(&raw_id) else {
                continue;
            };
            match WatchKind::parse(&kind_str) {
                Some(WatchKind::Coin) => coins.push(normalized),
                Some(WatchKind::P2) => p2s.push(normalized),
                None => {
                    tracing::warn!(
                        offer_id = clean_offer,
                        kind = kind_str.as_str(),
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
        let clean = offer_id.trim();
        if clean.is_empty() {
            return Ok(());
        }
        self.conn
            .execute(
                "DELETE FROM offer_coin_watches WHERE offer_id = ?1",
                params![clean],
            )
            .map_err(|err| db_err("offer_coin_watches clear", err))?;
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
        self.list_keys_for_market(market_id, WatchKind::Coin)
    }

    /// List distinct watched maker p2 hashes for a market (`kind = 'p2'` only).
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_watched_p2s_for_market(&self, market_id: &str) -> SignerResult<HashSet<String>> {
        self.list_keys_for_market(market_id, WatchKind::P2)
    }

    fn list_keys_for_market(
        &self,
        market_id: &str,
        kind: WatchKind,
    ) -> SignerResult<HashSet<String>> {
        let rows: Vec<String> = query_mapped(
            &self.conn,
            "SELECT DISTINCT coin_id FROM offer_coin_watches WHERE market_id = ?1 AND kind = ?2",
            params![market_id.trim(), kind.as_str()],
            "offer_coin_watches market keys",
            |row| row.get(0),
        )?;
        Ok(rows
            .into_iter()
            .filter_map(|value| watch_key(&value))
            .collect())
    }

    /// List all distinct durable maker p2 watch keys (process-wide WS filter union).
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_watched_p2s(&self) -> SignerResult<Vec<String>> {
        let rows: Vec<String> = query_mapped(
            &self.conn,
            "SELECT DISTINCT coin_id FROM offer_coin_watches WHERE kind = 'p2'",
            [],
            "offer_coin_watches p2 list",
            |row| row.get(0),
        )?;
        let mut out: Vec<String> = rows
            .into_iter()
            .filter_map(|value| watch_key(&value))
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// List distinct market ids watching any of the given coin/p2 keys.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_market_ids_for_watched_keys(&self, keys: &[String]) -> SignerResult<Vec<String>> {
        let normalized = unique_watch_keys(keys);
        if normalized.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT DISTINCT market_id FROM offer_coin_watches WHERE coin_id IN ({})",
            in_placeholders(normalized.len())
        );
        let mut markets: Vec<String> = query_mapped(
            &self.conn,
            &sql,
            rusqlite::params_from_iter(normalized.iter()),
            "offer_coin_watches market_id",
            |row| row.get(0),
        )?;
        markets.retain(|market| !market.trim().is_empty());
        markets.sort();
        Ok(markets)
    }

    /// Match offer state rows with the watch kind(s) that matched `keys`.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn match_watch_keys(&self, keys: &[String]) -> SignerResult<Vec<WatchHitRow>> {
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
        let normalized = unique_watch_keys(keys);
        if normalized.is_empty() {
            return Ok(HashMap::new());
        }
        let sql = format!(
            "SELECT DISTINCT offer_id, kind FROM offer_coin_watches WHERE coin_id IN ({})",
            in_placeholders(normalized.len())
        );
        let rows: Vec<(String, String)> = query_mapped(
            &self.conn,
            &sql,
            rusqlite::params_from_iter(normalized.iter()),
            "offer_coin_watches kind",
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut kind_by_offer: HashMap<String, WatchMatchKind> = HashMap::new();
        for (offer_id, kind_str) in rows {
            let Some(kind) = WatchKind::parse(&kind_str) else {
                tracing::warn!(
                    offer_id = %offer_id,
                    kind = %kind_str,
                    "skipping unknown offer_coin_watches kind in match query"
                );
                continue;
            };
            let match_kind = WatchMatchKind::from_watch_kind(kind);
            kind_by_offer
                .entry(offer_id)
                .and_modify(|existing| *existing = existing.merge(match_kind))
                .or_insert(match_kind);
        }
        Ok(kind_by_offer)
    }

    /// List offer ids watching a given coin id or p2.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` reads fail.
    pub fn list_offer_ids_for_watched_coin(&self, coin_id: &str) -> SignerResult<Vec<String>> {
        let Some(normalized) = watch_key(coin_id) else {
            return Ok(Vec::new());
        };
        let mut out: Vec<String> = query_mapped(
            &self.conn,
            "SELECT DISTINCT offer_id FROM offer_coin_watches WHERE coin_id = ?1",
            params![normalized],
            "offer_coin_watches offer_ids",
            |row| row.get(0),
        )?;
        out.retain(|offer_id| !offer_id.trim().is_empty());
        Ok(out)
    }
}
