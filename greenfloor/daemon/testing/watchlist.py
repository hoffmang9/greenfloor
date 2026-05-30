"""Rust-backed active-offer counting and coin watchlist patch points."""

from __future__ import annotations

from datetime import datetime
from pathlib import Path
from typing import Any

from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.runtime.offer_watchlist import (
    build_dexie_size_by_offer_id,
    match_watched_coin_ids,
    new_coin_watchlist_cache,
    set_watched_coin_ids_for_market,
    update_market_coin_watchlist_from_dexie,
)


def _watchlist_engine() -> Any:
    return import_engine()


def _active_offer_counts_by_size_engine() -> Any:
    return require_engine_method(
        _watchlist_engine(),
        "active_offer_counts_by_size",
        missing="watchlist active_offer_counts_by_size",
    )


def _active_offer_counts_by_size_and_side_engine() -> Any:
    return require_engine_method(
        _watchlist_engine(),
        "active_offer_counts_by_size_and_side",
        missing="watchlist active_offer_counts_by_size_and_side",
    )


def _db_path_from_store(store: Any) -> Path:
    db_path = getattr(store, "db_path", None)
    if isinstance(db_path, Path):
        return db_path
    if isinstance(db_path, str) and db_path.strip():
        return Path(db_path)
    raise TypeError("active_offer_counts requires SqliteStore with db_path")


def active_offer_counts_by_size(
    *,
    store: Any,
    market_id: str,
    clock: datetime,
    dexie_size_by_offer_id: dict[str, int] | None = None,
    tracked_sizes: set[int] | None = None,
) -> tuple[dict[int, int], dict[str, int], int]:
    payload = _active_offer_counts_by_size_engine()(
        _db_path_from_store(store),
        market_id,
        dexie_size_by_offer_id=dexie_size_by_offer_id,
        tracked_sizes=sorted(tracked_sizes) if tracked_sizes is not None else None,
        clock_iso=clock.isoformat(),
    )
    counts = {int(size): int(count) for size, count in dict(payload["counts_by_size"]).items()}
    state_counts = {str(k): int(v) for k, v in dict(payload["state_counts"]).items()}
    return counts, state_counts, int(payload["unmapped"])


def active_offer_counts_by_size_and_side(
    *,
    store: Any,
    market_id: str,
    clock: datetime,
    dexie_size_by_offer_id: dict[str, int] | None = None,
    tracked_sizes: set[int] | None = None,
) -> tuple[dict[str, dict[int, int]], dict[str, int], int]:
    payload = _active_offer_counts_by_size_and_side_engine()(
        _db_path_from_store(store),
        market_id,
        dexie_size_by_offer_id=dexie_size_by_offer_id,
        tracked_sizes=sorted(tracked_sizes) if tracked_sizes is not None else None,
        clock_iso=clock.isoformat(),
    )
    counts_by_side = {
        side: {int(size): int(count) for size, count in dict(sizes).items()}
        for side, sizes in dict(payload["counts_by_side"]).items()
    }
    state_counts = {str(k): int(v) for k, v in dict(payload["state_counts"]).items()}
    return counts_by_side, state_counts, int(payload["unmapped"])


__all__ = [
    "active_offer_counts_by_size",
    "active_offer_counts_by_size_and_side",
    "build_dexie_size_by_offer_id",
    "match_watched_coin_ids",
    "new_coin_watchlist_cache",
    "set_watched_coin_ids_for_market",
    "update_market_coin_watchlist_from_dexie",
]
