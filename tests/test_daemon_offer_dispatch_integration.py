"""Integration coverage for Rust daemon policy surfaces exposed via PyO3."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.daemon.cycle_market_batch import MarketDispatchState


def _engine():
    return import_engine()


def test_use_websocket_capture_for_trigger_mode_via_engine() -> None:
    use_ws = require_engine_method(
        _engine(),
        "use_websocket_capture_for_trigger_mode",
        missing="daemon websocket capture policy",
    )
    assert use_ws("websocket") is True
    assert use_ws("poll") is False
    assert use_ws("WebSocket") is True


def test_run_daemon_cycle_once_accepts_typed_request(tmp_path: Path) -> None:
    engine = _engine()
    request_cls = require_engine_method(
        engine,
        "DaemonRunOnceRequest",
        missing="daemon cycle request",
    )
    dispatch_cls = require_engine_method(
        engine,
        "DaemonDispatchState",
        missing="daemon dispatch state",
    )
    controls_cls = require_engine_method(
        engine,
        "DaemonCycleTestControls",
        missing="daemon cycle test controls",
    )
    run_once = require_engine_method(
        engine,
        "run_daemon_cycle_once",
        missing="daemon cycle",
    )
    program_path = tmp_path / "program.yaml"
    program_path.write_text("app:\n  network: mainnet\n  home_dir: /tmp/gf\n", encoding="utf-8")
    markets_path = tmp_path / "markets.yaml"
    markets_path.write_text("markets: []\n", encoding="utf-8")
    state_dir = tmp_path / "state"
    state_dir.mkdir()
    request = request_cls(
        program_path,
        markets_path,
        "https://api.coinset.org",
        state_dir,
        poll_coinset_mempool=False,
        use_websocket_capture=False,
        allowed_key_ids=[],
        dispatch_state=dispatch_cls(0, []),
        test_controls=controls_cls(),
    )
    try:
        response = run_once(request)
    except Exception as exc:
        assert "program_path is required" not in str(exc)
        return
    assert hasattr(response, "exit_code")
    assert hasattr(response, "dispatch_state")
    assert hasattr(response, "cycle_summary")


def test_daemon_dispatch_state_round_trip_via_engine_cycle(tmp_path: Path) -> None:
    from greenfloor.daemon.engine_cycle import run_daemon_cycle_once_via_engine

    captured: list[MarketDispatchState] = []
    dispatch_cls = require_engine_method(
        _engine(),
        "DaemonDispatchState",
        missing="daemon dispatch state",
    )

    def _fake_run(request: Any) -> object:
        dispatch = request.dispatch_state
        captured.append(
            MarketDispatchState(
                cursor=int(dispatch.cursor),
                immediate_requeue_ids=list(dispatch.immediate_requeue_ids),
            )
        )

        class _Response:
            exit_code = 0
            dispatch_state = dispatch_cls(1, ["m-new"])

        return _Response()

    dispatch = MarketDispatchState(cursor=2, immediate_requeue_ids=["m-old"])
    exit_code, updated = run_daemon_cycle_once_via_engine(
        program_path=tmp_path / "program.yaml",
        markets_path=tmp_path / "markets.yaml",
        testnet_markets_path=None,
        allowed_keys={"key-a"},
        db_path_override=None,
        coinset_base_url="https://api.coinset.org",
        state_dir=tmp_path / "state",
        poll_coinset_mempool=False,
        use_websocket_capture=True,
        market_dispatch_state=dispatch,
        run_fn=_fake_run,
    )
    assert exit_code == 0
    assert updated.cursor == 1
    assert updated.immediate_requeue_ids == ["m-new"]
    assert captured[0].cursor == 2
    assert captured[0].immediate_requeue_ids == ["m-old"]


def test_acquire_daemon_instance_lock_context_manager(tmp_path: Path) -> None:
    acquire = require_engine_method(
        _engine(),
        "acquire_daemon_instance_lock",
        missing="daemon instance lock",
    )
    state_dir = tmp_path / "state"
    with acquire(state_dir, "once"):
        lock_path = state_dir / "daemon.lock"
        assert lock_path.exists()
