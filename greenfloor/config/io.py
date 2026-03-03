from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

import yaml

from greenfloor.config.models import (
    MarketsConfig,
    ProgramConfig,
    parse_markets_config,
    parse_program_config,
)
from greenfloor.hex_utils import is_hex_id

_config_logger = logging.getLogger("greenfloor.config")


def _validate_base_markets_addresses(*, path: Path, raw: dict[str, Any]) -> None:
    rows = raw.get("markets")
    if not isinstance(rows, list):
        return
    bad_ids: list[str] = []
    for row in rows:
        if not isinstance(row, dict):
            continue
        receive_address = str(row.get("receive_address", "")).strip().lower()
        if receive_address.startswith("txch1"):
            bad_ids.append(str(row.get("id", "")).strip() or "<unknown>")
    if bad_ids:
        message = (
            f"testnet receive_address entries found in base markets config {path}; "
            "move these markets to testnet-markets.yaml"
        )
        _config_logger.error("%s market_ids=%s", message, ",".join(bad_ids))
        raise ValueError(message)


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
    raw = load_yaml(path)
    config = parse_program_config(raw)
    if config.app_log_level_was_missing:
        app = raw.get("app")
        if isinstance(app, dict):
            app["log_level"] = config.app_log_level
            write_yaml(path, raw)
    return config


def load_markets_config(path: Path) -> MarketsConfig:
    return load_markets_config_with_optional_overlay(path=path, overlay_path=None)


def load_markets_config_with_optional_overlay(
    *, path: Path, overlay_path: Path | None
) -> MarketsConfig:
    raw = load_yaml(path)
    _validate_base_markets_addresses(path=path, raw=raw)
    if overlay_path is not None:
        resolved_overlay = overlay_path.expanduser()
        if resolved_overlay.exists():
            overlay_raw = load_yaml(resolved_overlay)
            base_markets = raw.get("markets")
            overlay_markets = overlay_raw.get("markets")
            if not isinstance(base_markets, list):
                raise ValueError(f"markets must be a list in base config: {path}")
            if not isinstance(overlay_markets, list):
                raise ValueError(f"markets must be a list in overlay config: {resolved_overlay}")
            merged = dict(raw)
            merged["markets"] = [*base_markets, *overlay_markets]
            raw = merged
    return parse_markets_config(raw)


# ---------------------------------------------------------------------------
# Shared config-path helpers (used by both daemon and CLI)
# ---------------------------------------------------------------------------

_TESTNET_NETWORKS: frozenset[str] = frozenset({"testnet", "testnet11"})


def is_testnet(network: str) -> bool:
    return network.strip().lower() in _TESTNET_NETWORKS


def default_cats_config_path() -> Path | None:
    """Return the cats.yaml path, preferring ~/.greenfloor over the repo."""
    home_candidate = Path("~/.greenfloor/config/cats.yaml").expanduser()
    if home_candidate.exists():
        return home_candidate
    repo_candidate = Path("config/cats.yaml")
    if repo_candidate.exists():
        return repo_candidate
    return None


def default_state_dir_path() -> Path:
    """Return the canonical daemon state dir under ~/.greenfloor."""
    return Path("~/.greenfloor/state").expanduser()


def resolve_quote_asset_for_offer(*, quote_asset: str, network: str) -> str:
    """Resolve a quote asset identifier to its canonical form for offer building.

    Handles xch/txch network mapping and cats.yaml symbol lookup.
    """
    normalized = quote_asset.strip().lower()
    if normalized in {"xch", "txch", "1"}:
        return "txch" if is_testnet(network) else "xch"
    if is_hex_id(normalized):
        return normalized

    cats_path = default_cats_config_path()
    if cats_path is None:
        return quote_asset
    try:
        raw = load_yaml(cats_path)
    except Exception:
        return quote_asset
    if not isinstance(raw, dict):
        return quote_asset
    cats = raw.get("cats", [])
    if not isinstance(cats, list):
        return quote_asset
    for item in cats:
        if not isinstance(item, dict):
            continue
        symbol = str(item.get("base_symbol", "")).strip().lower()
        if symbol != normalized:
            continue
        asset_id = str(item.get("asset_id", "")).strip().lower()
        if is_hex_id(asset_id):
            return asset_id
    return quote_asset
