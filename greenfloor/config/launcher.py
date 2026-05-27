"""Vault launcher id resolution from program config."""

from __future__ import annotations

from pathlib import Path

from greenfloor.config.io import load_program_config
from greenfloor.hex_utils import normalize_hex_id


def launcher_id_from_program_config(program_config_path: str | Path) -> str:
    """Return normalized ``vault.launcher_id`` from ``program.yaml``."""
    cfg = load_program_config(Path(program_config_path).expanduser())
    vault = cfg.vault_config
    launcher = normalize_hex_id(vault.launcher_id) if vault is not None else ""
    if not launcher:
        raise RuntimeError("vault_launcher_id_missing_from_program_config")
    return launcher
