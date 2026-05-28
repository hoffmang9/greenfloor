"""Coinset websocket callback handlers for the daemon loop."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from greenfloor.daemon.watchlist import _match_watched_coin_ids
from greenfloor.storage.sqlite import SqliteStore


def ws_store_callback(db_path: Path, callback: Callable[[SqliteStore], None]) -> None:
    store = SqliteStore(db_path)
    try:
        callback(store)
    finally:
        store.close()


@dataclass(frozen=True, slots=True)
class CoinsetWebsocketHandlers:
    on_mempool_tx_ids: Callable[[list[str]], None]
    on_confirmed_tx_ids: Callable[[list[str]], None]
    on_audit_event: Callable[[str, dict[str, Any]], None]
    on_observed_coin_ids: Callable[[list[str]], None]


def build_coinset_websocket_handlers(*, db_path: Path) -> CoinsetWebsocketHandlers:
    def _with_store(callback: Callable[[SqliteStore], None]) -> None:
        ws_store_callback(db_path, callback)

    def _on_mempool_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return

        def _write(store: SqliteStore) -> None:
            new_count = store.observe_mempool_tx_ids(tx_ids)
            if new_count:
                store.add_audit_event(
                    "mempool_observed",
                    {"new_tx_ids": new_count, "source": "coinset_websocket"},
                )

        _with_store(_write)

    def _on_confirmed_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return

        def _write(store: SqliteStore) -> None:
            confirmed = store.confirm_tx_ids(tx_ids)
            store.add_audit_event(
                "tx_block_confirmed",
                {
                    "tx_ids": tx_ids,
                    "confirmed_count": confirmed,
                    "source": "coinset_websocket",
                },
            )

        _with_store(_write)

    def _on_audit_event(event_type: str, payload: dict[str, Any]) -> None:
        _with_store(lambda store: store.add_audit_event(event_type, payload))

    def _on_observed_coin_ids(coin_ids: list[str]) -> None:
        if not coin_ids:
            return
        hits = _match_watched_coin_ids(observed_coin_ids=coin_ids)
        if not hits:
            return

        def _write(store: SqliteStore) -> None:
            store.add_audit_event(
                "coin_watch_hit",
                {
                    "coin_id_count": len(coin_ids),
                    "coin_ids_sample": sorted({str(c).strip().lower() for c in coin_ids})[:10],
                    "market_hits": {market_id: ids[:10] for market_id, ids in hits.items()},
                    "source": "coinset_websocket",
                },
            )

        _with_store(_write)

    return CoinsetWebsocketHandlers(
        on_mempool_tx_ids=_on_mempool_tx_ids,
        on_confirmed_tx_ids=_on_confirmed_tx_ids,
        on_audit_event=_on_audit_event,
        on_observed_coin_ids=_on_observed_coin_ids,
    )
