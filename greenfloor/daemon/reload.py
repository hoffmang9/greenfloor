from __future__ import annotations

import json
from pathlib import Path


def reload_marker_path(state_dir: Path) -> Path:
    return state_dir / "reload_request.json"


def write_reload_marker(state_dir: Path) -> Path:
    state_dir.mkdir(parents=True, exist_ok=True)
    marker = reload_marker_path(state_dir)
    marker.write_text(json.dumps({"reload": True}), encoding="utf-8")
    return marker


def consume_reload_marker(state_dir: Path) -> bool:
    marker = reload_marker_path(state_dir)
    if not marker.exists():
        return False
    marker.unlink(missing_ok=True)
    return True
