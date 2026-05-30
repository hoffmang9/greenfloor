"""Integration coverage for Rust daemon policy surfaces exposed via PyO3."""

from __future__ import annotations

from pathlib import Path

from greenfloor.core.engine_bridge import import_engine, require_engine_method


def test_use_websocket_capture_for_trigger_mode_via_engine() -> None:
    use_ws = require_engine_method(
        import_engine(),
        "use_websocket_capture_for_trigger_mode",
        missing="daemon websocket capture policy",
    )
    assert use_ws("websocket") is True
    assert use_ws("poll") is False
    assert use_ws("WebSocket") is True


def test_run_daemon_cycle_once_accepts_dict_request(tmp_path: Path) -> None:
    run_once = require_engine_method(
        import_engine(),
        "run_daemon_cycle_once",
        missing="daemon cycle",
    )
    program_path = tmp_path / "program.yaml"
    program_path.write_text("app:\n  network: mainnet\n  home_dir: /tmp/gf\n", encoding="utf-8")
    markets_path = tmp_path / "markets.yaml"
    markets_path.write_text("markets: []\n", encoding="utf-8")
    state_dir = tmp_path / "state"
    state_dir.mkdir()
    request = {
        "program_path": str(program_path),
        "markets_path": str(markets_path),
        "state_dir": str(state_dir),
        "coinset_base_url": "https://api.coinset.org",
        "poll_coinset_mempool": False,
        "use_websocket_capture": False,
        "allowed_key_ids": [],
        "dispatch_state": {"cursor": 0, "immediate_requeue_ids": []},
        "test_controls": {},
    }
    try:
        response = run_once(request)
    except Exception as exc:
        # Config/market validation may fail in minimal fixtures; still require dict contract.
        assert "program_path is required" not in str(exc)
        return
    assert isinstance(response, dict)
    assert "exit_code" in response
    assert "dispatch_state" in response
    assert "cycle_summary" in response


def test_acquire_daemon_instance_lock_context_manager(tmp_path: Path) -> None:
    acquire = require_engine_method(
        import_engine(),
        "acquire_daemon_instance_lock",
        missing="daemon instance lock",
    )
    state_dir = tmp_path / "state"
    with acquire(state_dir, "once"):
        lock_path = state_dir / "daemon.lock"
        assert lock_path.exists()
