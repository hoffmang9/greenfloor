from __future__ import annotations

import json
from pathlib import Path

import pytest

from greenfloor.cli.offers_lifecycle import offers_cancel
from greenfloor.storage.sqlite import SqliteStore
from tests.helpers.offer_runtime_fixtures import write_manager_program


def _seed_state_db(db_path: Path, *, rows: list[tuple[str, str, str]]) -> None:
    db_path.parent.mkdir(parents=True, exist_ok=True)
    store = SqliteStore(db_path)
    try:
        for offer_id, market_id, state in rows:
            store.upsert_offer_state(
                offer_id=offer_id,
                market_id=market_id,
                state=state,
                last_seen_status=0,
            )
    finally:
        store.close()


def test_offers_cancel_cancel_open_uses_dexie(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    db_path = tmp_path / "db" / "greenfloor.sqlite"
    write_manager_program(program, tmp_path=tmp_path)
    _seed_state_db(
        db_path,
        rows=[
            ("offer-open", "m1", "open"),
            ("offer-expired", "m1", "expired"),
        ],
    )

    cancelled: list[str] = []

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def cancel_offer(self, offer_id: str) -> dict[str, object]:
            cancelled.append(offer_id)
            return {"success": True, "id": offer_id, "status": 3}

    monkeypatch.setattr("greenfloor.cli.offers_lifecycle.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.cli.offers_lifecycle.resolve_state_db_path",
        lambda **kwargs: db_path,
    )

    code = offers_cancel(
        program_path=program,
        offer_ids=[],
        cancel_open=True,
    )
    assert code == 0
    assert cancelled == ["offer-open"]
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] == "dexie"
    assert payload["selected_count"] == 1
    assert payload["cancelled_count"] == 1
    assert payload["failed_count"] == 0
    assert payload["items"][0]["offer_id"] == "offer-open"
    assert payload["items"][0]["result"]["success"] is True

    store = SqliteStore(db_path)
    try:
        rows = {row["offer_id"]: row for row in store.list_offer_states(limit=10)}
    finally:
        store.close()
    assert rows["offer-open"]["state"] == "cancelled"
    assert rows["offer-open"]["last_seen_status"] == 3
    assert rows["offer-expired"]["state"] == "expired"


def test_offers_cancel_by_offer_id_uses_dexie(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    db_path = tmp_path / "db" / "greenfloor.sqlite"
    write_manager_program(program, tmp_path=tmp_path)
    _seed_state_db(
        db_path,
        rows=[
            ("offer-target", "m1", "open"),
            ("offer-other", "m1", "open"),
        ],
    )

    cancelled: list[str] = []

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def cancel_offer(self, offer_id: str) -> dict[str, object]:
            cancelled.append(offer_id)
            return {"success": True, "id": offer_id, "status": 3}

    monkeypatch.setattr("greenfloor.cli.offers_lifecycle.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.cli.offers_lifecycle.resolve_state_db_path",
        lambda **kwargs: db_path,
    )

    code = offers_cancel(
        program_path=program,
        offer_ids=["offer-target"],
        cancel_open=False,
    )
    assert code == 0
    assert cancelled == ["offer-target"]
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["selected_count"] == 1
    assert payload["cancelled_count"] == 1
    assert payload["items"][0]["offer_id"] == "offer-target"


def test_offers_cancel_reports_dexie_failure(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    db_path = tmp_path / "db" / "greenfloor.sqlite"
    write_manager_program(program, tmp_path=tmp_path)
    _seed_state_db(db_path, rows=[("offer-fail", "m1", "open")])

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def cancel_offer(self, offer_id: str) -> dict[str, object]:
            _ = offer_id
            return {"success": False, "error": "not_found"}

    monkeypatch.setattr("greenfloor.cli.offers_lifecycle.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.cli.offers_lifecycle.resolve_state_db_path",
        lambda **kwargs: db_path,
    )

    code = offers_cancel(
        program_path=program,
        offer_ids=["offer-fail"],
        cancel_open=False,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["cancelled_count"] == 0
    assert payload["failed_count"] == 1
    assert payload["items"][0]["result"]["success"] is False
    assert payload["items"][0]["result"]["error"] == "not_found"


def test_offers_cancel_submit_onchain_after_offchain_removed(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program(program, tmp_path=tmp_path)

    with pytest.raises(
        ValueError,
        match="submit_onchain_after_offchain is removed with Cloud Wallet",
    ):
        offers_cancel(
            program_path=program,
            offer_ids=["offer-1"],
            cancel_open=False,
            submit_onchain_after_offchain=True,
        )
