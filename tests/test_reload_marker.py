import json
from pathlib import Path

from greenfloor.daemon.main import _consume_reload_marker


def _write_reload_marker(state_dir: Path) -> Path:
    state_dir.mkdir(parents=True, exist_ok=True)
    marker = state_dir / "reload_request.json"
    marker.write_text(json.dumps({"reload": True}), encoding="utf-8")
    return marker


def test_reload_marker_roundtrip(tmp_path: Path) -> None:
    state_dir = tmp_path / "state"
    marker = _write_reload_marker(state_dir)
    assert marker.exists()
    assert _consume_reload_marker(state_dir) is True
    assert marker.exists() is False
    assert _consume_reload_marker(state_dir) is False
