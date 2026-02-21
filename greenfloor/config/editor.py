from __future__ import annotations

import hashlib
import json
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import yaml


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def write_yaml_versioned(
    *,
    path: Path,
    data: dict[str, Any],
    actor: str,
    reason: str,
) -> dict[str, str | None]:
    """Atomically write YAML with versioned backup metadata.

    Behavior:
    - If the target exists, copy previous bytes into `.history/` with metadata.
    - Write new file atomically via temp file + replace.
    - Return checksums and backup metadata for audit/logging.
    """
    path.parent.mkdir(parents=True, exist_ok=True)
    history_dir = path.parent / ".history"
    history_dir.mkdir(parents=True, exist_ok=True)

    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    previous_bytes = path.read_bytes() if path.exists() else None
    previous_checksum = _sha256_bytes(previous_bytes) if previous_bytes is not None else None

    backup_file: Path | None = None
    backup_meta_file: Path | None = None
    if previous_bytes is not None:
        backup_file = history_dir / f"{path.name}.{timestamp}.bak.yaml"
        backup_file.write_bytes(previous_bytes)
        backup_meta_file = history_dir / f"{path.name}.{timestamp}.meta.json"
        backup_meta_file.write_text(
            json.dumps(
                {
                    "actor": actor,
                    "reason": reason,
                    "source_path": str(path),
                    "backup_path": str(backup_file),
                    "previous_checksum": previous_checksum,
                    "timestamp_utc": timestamp,
                },
                sort_keys=True,
            ),
            encoding="utf-8",
        )

    rendered = yaml.safe_dump(data, sort_keys=False).encode("utf-8")
    new_checksum = _sha256_bytes(rendered)

    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_bytes(rendered)
    tmp.replace(path)

    return {
        "new_checksum": new_checksum,
        "previous_checksum": previous_checksum,
        "backup_path": str(backup_file) if backup_file else None,
        "backup_meta_path": str(backup_meta_file) if backup_meta_file else None,
    }


def list_yaml_history(path: Path) -> list[dict[str, Any]]:
    history_dir = path.parent / ".history"
    if not history_dir.exists():
        return []
    entries: list[dict[str, Any]] = []
    pattern = f"{path.name}.*.meta.json"
    for meta_path in sorted(history_dir.glob(pattern), reverse=True):
        try:
            payload = json.loads(meta_path.read_text(encoding="utf-8"))
        except Exception:
            continue
        payload["_meta_path"] = str(meta_path)
        entries.append(payload)
    return entries


def latest_yaml_backup_path(path: Path) -> Path | None:
    history_dir = path.parent / ".history"
    if not history_dir.exists():
        return None
    pattern = f"{path.name}.*.bak.yaml"
    backups = sorted(history_dir.glob(pattern), reverse=True)
    if not backups:
        return None
    return backups[0]


def _backup_matches_target(path: Path, backup_path: Path) -> bool:
    """Ensure backup belongs to the target file's history namespace."""
    history_dir = (path.parent / ".history").resolve()
    backup_resolved = backup_path.resolve()
    if backup_resolved.parent != history_dir:
        return False
    # Backup file name format: <name>.<timestamp>.bak.yaml
    prefix = f"{path.name}."
    return backup_resolved.name.startswith(prefix) and backup_resolved.name.endswith(".bak.yaml")


def revert_yaml_from_backup(
    *,
    path: Path,
    backup_path: Path,
    actor: str,
    reason: str,
) -> dict[str, str | None]:
    if not backup_path.exists():
        raise ValueError(f"backup_path not found: {backup_path}")
    if not _backup_matches_target(path, backup_path):
        raise ValueError("backup_path does not belong to target config history namespace")
    loaded = yaml.safe_load(backup_path.read_text(encoding="utf-8")) or {}
    if not isinstance(loaded, dict):
        raise ValueError("backup yaml content must be a mapping")
    result = write_yaml_versioned(path=path, data=loaded, actor=actor, reason=reason)
    return result
