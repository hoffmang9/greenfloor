"""Integration coverage for Rust daemon policy surfaces exposed via PyO3."""

from __future__ import annotations

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
