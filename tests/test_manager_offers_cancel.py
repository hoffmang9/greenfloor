from __future__ import annotations

from pathlib import Path

from greenfloor.storage.sqlite import SqliteStore
from tests.helpers.dexie_http_mock import DexieHttpMock
from tests.helpers.manager_cli import parse_json_output, run_manager
from tests.helpers.manager_program_fixtures import write_manager_program

_DEFAULT_DEXIE_BASE = "https://api.dexie.space"


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


def _run_offers_cancel_with_mock(
    *,
    program: Path,
    dexie: DexieHttpMock,
    offer_ids: list[str],
    cancel_open: bool,
) -> tuple[int, dict]:
    base_url = dexie.start()
    original = program.read_text(encoding="utf-8")
    program.write_text(original.replace(_DEFAULT_DEXIE_BASE, base_url), encoding="utf-8")
    try:
        argv = [
            "--program-config",
            str(program),
            "offers-cancel",
        ]
        if cancel_open:
            argv.append("--cancel-open")
        for offer_id in offer_ids:
            argv.extend(["--offer-id", offer_id])
        code, stdout, _stderr = run_manager(argv)
        return code, parse_json_output(stdout)
    finally:
        dexie.stop()
        program.write_text(original, encoding="utf-8")


def test_offers_cancel_cancel_open_uses_dexie(tmp_path: Path) -> None:
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

    dexie = DexieHttpMock()
    dexie.set_offers({"offer-open": {"id": "offer-open", "status": 0}})
    code, payload = _run_offers_cancel_with_mock(
        program=program,
        dexie=dexie,
        offer_ids=[],
        cancel_open=True,
    )
    assert code == 0
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


def test_offers_cancel_by_offer_id_uses_dexie(tmp_path: Path) -> None:
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

    dexie = DexieHttpMock()
    dexie.set_offers({"offer-target": {"id": "offer-target", "status": 0}})
    code, payload = _run_offers_cancel_with_mock(
        program=program,
        dexie=dexie,
        offer_ids=["offer-target"],
        cancel_open=False,
    )
    assert code == 0
    assert payload["selected_count"] == 1
    assert payload["cancelled_count"] == 1
    assert payload["items"][0]["offer_id"] == "offer-target"


def test_offers_cancel_reports_dexie_failure(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    db_path = tmp_path / "db" / "greenfloor.sqlite"
    write_manager_program(program, tmp_path=tmp_path)
    _seed_state_db(db_path, rows=[("offer-fail", "m1", "open")])

    dexie = DexieHttpMock()
    dexie.set_cancel_failure("offer-fail", "not_found")
    code, payload = _run_offers_cancel_with_mock(
        program=program,
        dexie=dexie,
        offer_ids=["offer-fail"],
        cancel_open=False,
    )
    assert code == 2
    assert payload["cancelled_count"] == 0
    assert payload["failed_count"] == 1
    assert payload["items"][0]["result"]["success"] is False
    assert payload["items"][0]["result"]["error"] == "not_found"


def test_offers_cancel_rejects_removed_submit_onchain_flag(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    code, _stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "offers-cancel",
            "--offer-id",
            "offer-1",
            "--submit-onchain-after-offchain",
        ]
    )
    assert code != 0
