"""Vault launcher id resolution from program config."""

from __future__ import annotations

from pathlib import Path

from greenfloor.config.io import load_yaml
from greenfloor.hex_utils import normalize_hex_id


def launcher_id_from_program_config(program_config_path: str | Path) -> str:
    """Return normalized ``vault.launcher_id`` from ``program.yaml``."""
    raw = load_yaml(Path(program_config_path).expanduser())
    vault = raw.get("vault")
    if not isinstance(vault, dict):
        raise RuntimeError("vault_launcher_id_missing_from_program_config")
    launcher = normalize_hex_id(str(vault.get("launcher_id", "")))
    if not launcher:
        raise RuntimeError("vault_launcher_id_missing_from_program_config")
    return launcher
