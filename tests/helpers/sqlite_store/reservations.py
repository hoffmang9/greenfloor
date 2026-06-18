from __future__ import annotations

from datetime import UTC, datetime

from tests.helpers.sqlite_store.base import SqliteStoreMixin
from tests.helpers.sqlite_store.schema import utcnow_iso


class ReservationStoreMixin(SqliteStoreMixin):

    def add_offer_reservation_lease(
        self,
        *,
        reservation_id: str,
        market_id: str,
        wallet_id: str,
        asset_amounts: dict[str, int],
        lease_seconds: int,
    ) -> None:
        if not reservation_id:
            raise ValueError("reservation_id is required")
        if lease_seconds <= 0:
            raise ValueError("lease_seconds must be > 0")
        if not asset_amounts:
            raise ValueError("asset_amounts must be non-empty")
        created_at = utcnow_iso()
        expires_at = datetime.now(UTC).timestamp() + float(lease_seconds)
        expires_at_iso = datetime.fromtimestamp(expires_at, UTC).isoformat()
        rows = [
            (
                reservation_id,
                market_id,
                wallet_id,
                str(asset_id),
                int(amount),
                "active",
                created_at,
                expires_at_iso,
                None,
            )
            for asset_id, amount in asset_amounts.items()
            if int(amount) > 0
        ]
        if not rows:
            raise ValueError("asset_amounts must contain positive amounts")
        self.conn.executemany(
            """
            INSERT INTO offer_reservation_lease
              (reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            rows,
        )
        self.conn.commit()

    def try_acquire_offer_reservation_lease(
        self,
        *,
        reservation_id: str,
        market_id: str,
        wallet_id: str,
        requested_amounts: dict[str, int],
        available_amounts: dict[str, int],
        lease_seconds: int,
        now: datetime | None = None,
    ) -> str | None:
        if not reservation_id:
            raise ValueError("reservation_id is required")
        if lease_seconds <= 0:
            raise ValueError("lease_seconds must be > 0")
        normalized_requests = {
            str(asset_id).strip().lower(): int(amount)
            for asset_id, amount in requested_amounts.items()
            if int(amount) > 0
        }
        if not normalized_requests:
            return "reservation_empty_request"
        normalized_available = {
            str(asset_id).strip().lower(): int(amount)
            for asset_id, amount in available_amounts.items()
            if int(amount) > 0
        }
        now_dt = now or datetime.now(UTC)
        now_iso = now_dt.isoformat()
        expires_at_iso = datetime.fromtimestamp(
            now_dt.timestamp() + float(lease_seconds), UTC
        ).isoformat()
        created_at_iso = now_iso
        try:
            self.conn.execute("BEGIN IMMEDIATE")
            self.conn.execute(
                """
                UPDATE offer_reservation_lease
                SET status = 'expired',
                    released_at = COALESCE(released_at, ?)
                WHERE status = 'active'
                  AND expires_at <= ?
                """,
                (now_iso, now_iso),
            )
            rows = self.conn.execute(
                """
                SELECT asset_id, COALESCE(SUM(amount), 0) AS reserved_amount
                FROM offer_reservation_lease
                WHERE wallet_id = ?
                  AND status = 'active'
                  AND expires_at > ?
                GROUP BY asset_id
                """,
                (wallet_id, now_iso),
            ).fetchall()
            reserved_by_asset = {
                str(row["asset_id"]).strip().lower(): int(row["reserved_amount"] or 0)
                for row in rows
            }
            for asset_id, amount in normalized_requests.items():
                available = int(normalized_available.get(asset_id, 0))
                already_reserved = int(reserved_by_asset.get(asset_id, 0))
                if available - already_reserved < amount:
                    self.conn.rollback()
                    return (
                        f"reservation_insufficient_{asset_id}:"
                        f"available={available}:reserved={already_reserved}:needed={amount}"
                    )
            insert_rows = [
                (
                    reservation_id,
                    market_id,
                    wallet_id,
                    str(asset_id),
                    int(amount),
                    "active",
                    created_at_iso,
                    expires_at_iso,
                    None,
                )
                for asset_id, amount in normalized_requests.items()
            ]
            self.conn.executemany(
                """
                INSERT INTO offer_reservation_lease
                  (reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                insert_rows,
            )
            self.conn.commit()
            return None
        except Exception:
            self.conn.rollback()
            raise

    def release_offer_reservation_lease(
        self,
        *,
        reservation_id: str,
        release_status: str,
    ) -> int:
        released_at = utcnow_iso()
        cur = self.conn.execute(
            """
            UPDATE offer_reservation_lease
            SET status = ?, released_at = ?
            WHERE reservation_id = ?
              AND status = 'active'
            """,
            (release_status, released_at, reservation_id),
        )
        self.conn.commit()
        return int(cur.rowcount or 0)

    def expire_offer_reservation_leases(self, *, now: datetime | None = None) -> int:
        now_iso = (now or datetime.now(UTC)).isoformat()
        cur = self.conn.execute(
            """
            UPDATE offer_reservation_lease
            SET status = 'expired',
                released_at = COALESCE(released_at, ?)
            WHERE status = 'active'
              AND expires_at <= ?
            """,
            (now_iso, now_iso),
        )
        self.conn.commit()
        return int(cur.rowcount or 0)

    def prune_offer_reservation_leases(self, *, older_than: datetime) -> int:
        cutoff_iso = older_than.astimezone(UTC).isoformat()
        cur = self.conn.execute(
            """
            DELETE FROM offer_reservation_lease
            WHERE status != 'active'
              AND COALESCE(released_at, expires_at) < ?
            """,
            (cutoff_iso,),
        )
        self.conn.commit()
        return int(cur.rowcount or 0)

    def get_offer_reserved_amounts_by_asset(self, *, wallet_id: str) -> dict[str, int]:
        rows = self.conn.execute(
            """
            SELECT asset_id, COALESCE(SUM(amount), 0) AS reserved_amount
            FROM offer_reservation_lease
            WHERE wallet_id = ?
              AND status = 'active'
              AND expires_at > ?
            GROUP BY asset_id
            """,
            (wallet_id, utcnow_iso()),
        ).fetchall()
        return {str(row["asset_id"]): int(row["reserved_amount"] or 0) for row in rows}

    def list_offer_reservation_leases(
        self,
        *,
        reservation_id: str | None = None,
        include_inactive: bool = True,
    ) -> list[dict[str, str | int | None]]:
        where_clauses: list[str] = []
        params: list[object] = []
        if reservation_id:
            where_clauses.append("reservation_id = ?")
            params.append(reservation_id)
        if not include_inactive:
            where_clauses.append("status = 'active'")
            where_clauses.append("expires_at > ?")
            params.append(utcnow_iso())
        where_sql = ""
        if where_clauses:
            where_sql = "WHERE " + " AND ".join(where_clauses)
        rows = self.conn.execute(
            f"""
            SELECT reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at
            FROM offer_reservation_lease
            {where_sql}
            ORDER BY id ASC
            """,
            params,
        ).fetchall()
        return [
            {
                "reservation_id": str(row["reservation_id"]),
                "market_id": str(row["market_id"]),
                "wallet_id": str(row["wallet_id"]),
                "asset_id": str(row["asset_id"]),
                "amount": int(row["amount"] or 0),
                "status": str(row["status"]),
                "created_at": str(row["created_at"]),
                "expires_at": str(row["expires_at"]),
                "released_at": str(row["released_at"]) if row["released_at"] is not None else None,
            }
            for row in rows
        ]
