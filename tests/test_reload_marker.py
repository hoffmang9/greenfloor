from pathlib import Path

from greenfloor.daemon.reload import consume_reload_marker, write_reload_marker


def test_reload_marker_roundtrip(tmp_path: Path) -> None:
    state_dir = tmp_path / "state"
    marker = write_reload_marker(state_dir)
    assert marker.exists()
    assert consume_reload_marker(state_dir) is True
    assert marker.exists() is False
    assert consume_reload_marker(state_dir) is False
