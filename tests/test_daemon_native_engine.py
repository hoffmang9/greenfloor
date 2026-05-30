from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

from greenfloor.cli.engine_binary import daemon_run_once_argv, resolve_greenfloor_engine_binary
from tests.helpers.daemon_websocket_fixtures import write_markets, write_program


def test_daemon_run_once_argv_includes_paths(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    state_dir = tmp_path / "state"
    argv = daemon_run_once_argv(
        binary=Path("/tmp/greenfloor-engine"),
        program_path=program,
        markets_path=markets,
        testnet_markets_path=tmp_path / "testnet.yaml",
        key_ids="key-a,key-b",
        state_db=str(tmp_path / "db.sqlite"),
        coinset_base_url="http://coinset.local",
        state_dir=state_dir,
    )
    assert argv[:4] == ["/tmp/greenfloor-engine", "daemon", "run-once", "--program-config"]
    assert str(program) in argv
    assert str(markets) in argv
    assert "--key-ids" in argv
    assert "key-a,key-b" in argv


def test_bridge_subprocess_run_cycle_preamble_roundtrip(tmp_path: Path) -> None:
    home = tmp_path / "home"
    home.mkdir()
    program = tmp_path / "program.yaml"
    write_program(program, home)

    request = {
        "method": "run_cycle_preamble",
        "kwargs": {
            "program_path": str(program),
            "db_path": str(tmp_path / "state.sqlite"),
            "coinset_base_url": "http://coinset.local",
            "poll_coinset_mempool": False,
            "use_websocket_capture": False,
        },
    }
    completed = subprocess.run(
        [sys.executable, "-m", "greenfloor.daemon.bridge_subprocess"],
        input=json.dumps(request),
        text=True,
        capture_output=True,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr
    payload = json.loads(completed.stdout)
    assert payload["ok"] is True
    assert "cycle_error_count" in payload["result"]
    assert "xch_price_usd" in payload["result"]


def _engine_binary_available() -> bool:
    try:
        return resolve_greenfloor_engine_binary().is_file()
    except Exception:
        return False


@pytest.mark.skipif(not _engine_binary_available(), reason="greenfloor-engine binary not built")
def test_engine_daemon_run_once_lock_conflict(tmp_path: Path) -> None:
    from greenfloor.daemon.main import _acquire_daemon_instance_lock

    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program, home)
    write_markets(markets)

    binary = resolve_greenfloor_engine_binary()
    argv = daemon_run_once_argv(
        binary=binary,
        program_path=program,
        markets_path=markets,
        testnet_markets_path=None,
        key_ids=None,
        state_db=str(tmp_path / "state.sqlite"),
        coinset_base_url="http://coinset.local",
        state_dir=state_dir,
    )
    with _acquire_daemon_instance_lock(state_dir=state_dir, mode="loop"):
        completed = subprocess.run(argv, check=False, env={"GREENFLOOR_PYTHON": sys.executable})
        assert completed.returncode == 3
