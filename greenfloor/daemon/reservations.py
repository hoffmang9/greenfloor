from __future__ import annotations

import threading
import uuid
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path

from greenfloor.storage.sqlite import SqliteStore


@dataclass(frozen=True, slots=True)
class ReservationAcquireResult:
    ok: bool
    reservation_id: str | None
    error: str | None = None


class AssetReservationCoordinator:
    """Thread-safe amount-based lease coordinator for offer reservations."""

    def __init__(
        self,
        *,
        db_path: Path,
        lease_seconds: int,
        retention_seconds: int = 7 * 24 * 60 * 60,
    ) -> None:
        self._db_path = Path(db_path)
        self._lease_seconds = max(30, int(lease_seconds))
        self._retention_seconds = max(3600, int(retention_seconds))
        self._lock = threading.Lock()

    def _open_store(self) -> SqliteStore:
        return SqliteStore(self._db_path)

    def expire_stale(self) -> int:
        with self._lock:
            store = self._open_store()
            try:
                now = datetime.now(UTC)
                expired = store.expire_offer_reservation_leases(now=now)
                prune_before = datetime.fromtimestamp(
                    now.timestamp() - float(self._retention_seconds), UTC
                )
                store.prune_offer_reservation_leases(older_than=prune_before)
                return expired
            finally:
                store.close()

    def try_acquire(
        self,
        *,
        market_id: str,
        wallet_id: str,
        requested_amounts: dict[str, int],
        available_amounts: dict[str, int],
    ) -> ReservationAcquireResult:
        normalized_requests = {
            str(asset_id).strip().lower(): int(amount)
            for asset_id, amount in requested_amounts.items()
            if int(amount) > 0
        }
        if not normalized_requests:
            return ReservationAcquireResult(
                ok=False, reservation_id=None, error="reservation_empty_request"
            )
        normalized_available = {
            str(asset_id).strip().lower(): int(amount)
            for asset_id, amount in available_amounts.items()
            if int(amount) > 0
        }
        with self._lock:
            store = self._open_store()
            try:
                reservation_id = f"res-{uuid.uuid4().hex}"
                acquire_error = store.try_acquire_offer_reservation_lease(
                    reservation_id=reservation_id,
                    market_id=market_id,
                    wallet_id=wallet_id,
                    requested_amounts=normalized_requests,
                    available_amounts=normalized_available,
                    lease_seconds=self._lease_seconds,
                )
                if acquire_error is not None:
                    return ReservationAcquireResult(
                        ok=False,
                        reservation_id=None,
                        error=acquire_error,
                    )
                return ReservationAcquireResult(ok=True, reservation_id=reservation_id, error=None)
            finally:
                store.close()

    def release(self, *, reservation_id: str, status: str) -> int:
        with self._lock:
            store = self._open_store()
            try:
                return store.release_offer_reservation_lease(
                    reservation_id=reservation_id,
                    release_status=status,
                )
            finally:
                store.close()
