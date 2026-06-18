"""SQLite persistence helpers for integration tests.

Canonical schema owner: ``greenfloor-engine/src/storage/schema.rs`` and
``greenfloor-engine/src/storage/sqlite/``. Keep this package aligned with Rust
storage when adding columns or tables.
"""

from __future__ import annotations

import sqlite3
from pathlib import Path

from tests.helpers.sqlite_store.alerts import AlertStoreMixin, StoredAlertState
from tests.helpers.sqlite_store.audit import AuditStoreMixin
from tests.helpers.sqlite_store.coin_ops import CoinOpStoreMixin
from tests.helpers.sqlite_store.offers import OfferStoreMixin
from tests.helpers.sqlite_store.pricing import PricingStoreMixin
from tests.helpers.sqlite_store.reservations import ReservationStoreMixin
from tests.helpers.sqlite_store.schema import SCHEMA_SQL
from tests.helpers.sqlite_store.tx_signals import TxSignalStoreMixin

__all__ = ["SqliteStore", "StoredAlertState"]


class SqliteStore(
    AlertStoreMixin,
    AuditStoreMixin,
    PricingStoreMixin,
    TxSignalStoreMixin,
    OfferStoreMixin,
    CoinOpStoreMixin,
    ReservationStoreMixin,
):
    def __init__(self, db_path: Path) -> None:
        self.db_path = db_path
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        # Parallel market workers open independent connections. Use a non-zero
        # lock wait so short write-contention windows do not fail immediately.
        self.conn = sqlite3.connect(self.db_path, timeout=30.0)
        self.conn.row_factory = sqlite3.Row
        self.conn.execute("PRAGMA busy_timeout = 30000")
        self._init_schema()

    def close(self) -> None:
        self.conn.close()

    def _init_schema(self) -> None:
        self.conn.executescript(SCHEMA_SQL)
        self.conn.commit()
