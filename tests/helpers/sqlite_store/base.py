from __future__ import annotations

import sqlite3


class SqliteStoreMixin:
    conn: sqlite3.Connection
