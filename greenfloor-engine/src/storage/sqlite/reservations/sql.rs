use std::collections::BTreeMap;

use rusqlite::{params, Connection, Row, Rows};

use crate::error::{SignerError, SignerResult};

use super::types::{normalize_asset_id, OfferReservationLeaseRow};

pub(super) fn db_err(context: &str, err: impl std::fmt::Display) -> SignerError {
    SignerError::Other(format!("{context}: {err}"))
}

const EXPIRE_STALE_LEASES_SQL: &str = r"
UPDATE offer_reservation_lease
SET status = 'expired',
    released_at = COALESCE(released_at, ?1)
WHERE status = 'active'
  AND expires_at <= ?2
";

const INSERT_ACTIVE_LEASE_SQL: &str = r"
INSERT INTO offer_reservation_lease
  (reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at)
VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, NULL)
";

const ACTIVE_RESERVED_BY_ASSET_SQL: &str = r"
SELECT asset_id, COALESCE(SUM(amount), 0) AS reserved_amount
FROM offer_reservation_lease
WHERE wallet_id = ?1
  AND status = 'active'
  AND expires_at > ?2
GROUP BY asset_id
";

const LIST_LEASES_SQL: &str = r"
SELECT reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at
FROM offer_reservation_lease
ORDER BY id ASC
";

const LIST_LEASES_BY_ID_SQL: &str = r"
SELECT reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at
FROM offer_reservation_lease
WHERE reservation_id = ?1
ORDER BY id ASC
";

const RELEASE_LEASE_SQL: &str = r"
UPDATE offer_reservation_lease
SET status = ?1, released_at = ?2
WHERE reservation_id = ?3
  AND status = 'active'
";

const PRUNE_INACTIVE_LEASES_SQL: &str = r"
DELETE FROM offer_reservation_lease
WHERE status != 'active'
  AND COALESCE(released_at, expires_at) < ?1
";

pub(super) fn expire_stale_leases(conn: &Connection, now_iso: &str) -> SignerResult<usize> {
    conn.execute(EXPIRE_STALE_LEASES_SQL, params![now_iso, now_iso])
        .map_err(|err| db_err("failed to expire stale reservation leases", err))
}

/// Insert one row per asset. `asset_amounts` keys must already be normalized asset IDs
/// (trimmed lowercase), e.g. from [`super::types::positive_asset_amounts`].
pub(super) fn insert_active_leases(
    conn: &Connection,
    reservation_id: &str,
    market_id: &str,
    wallet_id: &str,
    asset_amounts: &BTreeMap<String, i64>,
    created_at: &str,
    expires_at: &str,
) -> SignerResult<()> {
    for (asset_id, amount) in asset_amounts {
        conn.execute(
            INSERT_ACTIVE_LEASE_SQL,
            params![
                reservation_id,
                market_id,
                wallet_id,
                asset_id,
                amount,
                created_at,
                expires_at,
            ],
        )
        .map_err(|err| db_err("failed to insert reservation lease", err))?;
    }
    Ok(())
}

pub(super) fn query_active_reserved_by_asset(
    conn: &Connection,
    wallet_id: &str,
    now_iso: &str,
) -> SignerResult<BTreeMap<String, i64>> {
    let mut stmt = conn
        .prepare(ACTIVE_RESERVED_BY_ASSET_SQL)
        .map_err(|err| db_err("failed to prepare reserved amounts query", err))?;
    let mut rows = stmt
        .query(params![wallet_id, now_iso])
        .map_err(|err| db_err("failed to query reserved amounts", err))?;
    let mut out = BTreeMap::default();
    while let Some(row) = rows
        .next()
        .map_err(|err| db_err("failed to read reserved amount row", err))?
    {
        let asset_id: String = row
            .get(0)
            .map_err(|err| db_err("failed to read asset_id", err))?;
        let amount: i64 = row
            .get(1)
            .map_err(|err| db_err("failed to read reserved_amount", err))?;
        out.insert(normalize_asset_id(&asset_id), amount);
    }
    Ok(out)
}

pub(super) fn release_lease(
    conn: &Connection,
    reservation_id: &str,
    release_status: &str,
    released_at: &str,
) -> SignerResult<usize> {
    conn.execute(
        RELEASE_LEASE_SQL,
        params![release_status, released_at, reservation_id],
    )
    .map_err(|err| db_err("failed to release reservation lease", err))
}

pub(super) fn prune_inactive_leases(conn: &Connection, cutoff_iso: &str) -> SignerResult<usize> {
    conn.execute(PRUNE_INACTIVE_LEASES_SQL, params![cutoff_iso])
        .map_err(|err| db_err("failed to prune reservation leases", err))
}

fn read_lease_row(row: &Row<'_>) -> SignerResult<OfferReservationLeaseRow> {
    Ok(OfferReservationLeaseRow {
        reservation_id: row
            .get(0)
            .map_err(|err| db_err("failed to read reservation_id", err))?,
        market_id: row
            .get(1)
            .map_err(|err| db_err("failed to read market_id", err))?,
        wallet_id: row
            .get(2)
            .map_err(|err| db_err("failed to read wallet_id", err))?,
        asset_id: row
            .get(3)
            .map_err(|err| db_err("failed to read asset_id", err))?,
        amount: row
            .get(4)
            .map_err(|err| db_err("failed to read amount", err))?,
        status: row
            .get(5)
            .map_err(|err| db_err("failed to read status", err))?,
        created_at: row
            .get(6)
            .map_err(|err| db_err("failed to read created_at", err))?,
        expires_at: row
            .get(7)
            .map_err(|err| db_err("failed to read expires_at", err))?,
        released_at: row.get(8).ok(),
    })
}

fn collect_lease_rows(rows: &mut Rows<'_>) -> SignerResult<Vec<OfferReservationLeaseRow>> {
    let mut out = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|err| db_err("failed to read reservation lease row", err))?
    {
        out.push(read_lease_row(row)?);
    }
    Ok(out)
}

fn query_lease_rows(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> SignerResult<Vec<OfferReservationLeaseRow>> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|err| db_err("failed to prepare reservation lease query", err))?;
    let mut rows = stmt
        .query(params)
        .map_err(|err| db_err("failed to query reservation leases", err))?;
    collect_lease_rows(&mut rows)
}

pub(super) fn list_leases(
    conn: &Connection,
    reservation_id: Option<&str>,
) -> SignerResult<Vec<OfferReservationLeaseRow>> {
    let reservation_id = reservation_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(reservation_id) = reservation_id {
        query_lease_rows(conn, LIST_LEASES_BY_ID_SQL, params![reservation_id])
    } else {
        query_lease_rows(conn, LIST_LEASES_SQL, [])
    }
}
