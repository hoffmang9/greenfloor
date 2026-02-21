from __future__ import annotations

from pathlib import Path
from typing import Any

import yaml

from greenfloor.config.models import (
    MarketsConfig,
    ProgramConfig,
    parse_markets_config,
    parse_program_config,
)


def load_yaml(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as f:
        data = yaml.safe_load(f) or {}
    if not isinstance(data, dict):
        raise ValueError(f"YAML file must parse to a mapping: {path}")
    return data


def write_yaml(path: Path, data: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        yaml.safe_dump(data, f, sort_keys=False)


def load_program_config(path: Path) -> ProgramConfig:
    return parse_program_config(load_yaml(path))


def load_markets_config(path: Path) -> MarketsConfig:
    return parse_markets_config(load_yaml(path))
