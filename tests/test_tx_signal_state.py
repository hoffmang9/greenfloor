from pathlib import Path

from greenfloor.storage.sqlite import SqliteStore


def test_tx_signal_observe_and_confirm(tmp_path: Path) -> None:
    db = tmp_path / "greenfloor.sqlite"
    store = SqliteStore(db)
    try:
        inserted = store.observe_mempool_tx_ids(["a", "b", "a"])
        assert inserted == 2
        updated = store.confirm_tx_ids(["a", "x"])
        assert updated == 1
    finally:
        store.close()
