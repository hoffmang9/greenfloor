"""Vault launcher id resolution from program config."""

from __future__ import annotations

from pathlib import Path

from greenfloor.config.io import load_program_fields
from greenfloor.hex_utils import normalize_hex_id


def launcher_id_from_program_config(program_config_path: str | Path) -> str:
    """Return normalized ``vault.launcher_id`` from ``program.yaml``."""
    fields = load_program_fields(program_config=Path(program_config_path).expanduser())
    launcher = normalize_hex_id(str(fields.get("vault_launcher_id", "")))
    if not launcher:
        raise RuntimeError("vault_launcher_id_missing_from_program_config")
    return launcher
