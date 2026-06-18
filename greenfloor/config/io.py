from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml

from greenfloor.engine_binary import resolve_greenfloor_manager_binary
from greenfloor.hex_utils import is_hex_id

_TESTNET_NETWORKS: frozenset[str] = frozenset({"testnet", "testnet11"})

_DEFAULT_PROGRAM_CONFIG = Path("~/.greenfloor/config/program.yaml")
_DEFAULT_MARKETS_CONFIG = Path("~/.greenfloor/config/markets.yaml")
_DEFAULT_TESTNET_MARKETS_CONFIG = Path("~/.greenfloor/config/testnet-markets.yaml")


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


def is_testnet(network: str) -> bool:
    return network.strip().lower() in _TESTNET_NETWORKS


def load_markets_yaml(path: Path) -> dict[str, Any]:
    return load_markets_yaml_with_optional_overlay(path=path, overlay_path=None)


def load_markets_yaml_with_optional_overlay(
    *, path: Path, overlay_path: Path | None
) -> dict[str, Any]:
    raw = load_yaml(path)
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
    return raw


@dataclass(frozen=True, slots=True)
class ScriptProgramFields:
    network: str
    home_dir: str
    signer_kms_key_id: str
    signer_kms_region: str
    signer_key_registry: dict[str, dict[str, Any]]

    @classmethod
    def from_raw(cls, raw: dict[str, Any]) -> ScriptProgramFields:
        app = raw.get("app")
        network = "mainnet"
        home_dir = "~/.greenfloor"
        if isinstance(app, dict):
            network = str(app.get("network", network)).strip() or network
            home_dir = str(app.get("home_dir", home_dir)).strip() or home_dir

        signer = raw.get("signer")
        signer_kms_key_id = ""
        signer_kms_region = "us-west-2"
        if isinstance(signer, dict):
            signer_kms_key_id = str(signer.get("kms_key_id", "")).strip()
            region = str(signer.get("kms_region", "")).strip()
            if region:
                signer_kms_region = region

        keys = raw.get("keys")
        registry: dict[str, dict[str, Any]] = {}
        if isinstance(keys, dict):
            rows = keys.get("registry")
            if isinstance(rows, list):
                for row in rows:
                    if not isinstance(row, dict):
                        continue
                    key_id = str(row.get("key_id", "")).strip()
                    if key_id:
                        registry[key_id] = row

        return cls(
            network=network,
            home_dir=home_dir,
            signer_kms_key_id=signer_kms_key_id,
            signer_kms_region=signer_kms_region,
            signer_key_registry=registry,
        )


def enabled_market_rows(raw: dict[str, Any]) -> list[dict[str, Any]]:
    markets = raw.get("markets")
    if not isinstance(markets, list):
        return []
    return [row for row in markets if isinstance(row, dict) and bool(row.get("enabled"))]


def run_program_config_validate(*, program_config: Path) -> int:
    """Validate program.yaml only via native ``greenfloor-manager program-config-validate``."""
    argv = [
        str(resolve_greenfloor_manager_binary()),
        "--program-config",
        str(program_config),
        "program-config-validate",
    ]
    completed = subprocess.run(argv, check=False)
    return int(completed.returncode)


def ensure_program_config_valid(*, program_config: Path | None = None) -> None:
    """Run native program-only validation using the default path when omitted."""
    program_path = (program_config or _DEFAULT_PROGRAM_CONFIG).expanduser()
    code = run_program_config_validate(program_config=program_path)
    if code != 0:
        raise SystemExit(code)


def run_config_validate(
    *,
    program_config: Path,
    markets_config: Path,
    testnet_markets_config: Path | None = None,
) -> int:
    """Validate operator config via native ``greenfloor-manager config-validate``."""
    argv = [
        str(resolve_greenfloor_manager_binary()),
        "--program-config",
        str(program_config),
        "--markets-config",
        str(markets_config),
        "config-validate",
    ]
    if testnet_markets_config is not None:
        argv[5:5] = [
            "--testnet-markets-config",
            str(testnet_markets_config),
        ]
    completed = subprocess.run(argv, check=False)
    return int(completed.returncode)


def ensure_operator_config_valid(
    *,
    program_config: Path | None = None,
    markets_config: Path | None = None,
    testnet_markets_config: Path | None = None,
) -> None:
    """Run native config validation using default operator paths when omitted."""
    program_path = (program_config or _DEFAULT_PROGRAM_CONFIG).expanduser()
    markets_path = (markets_config or _DEFAULT_MARKETS_CONFIG).expanduser()
    overlay = testnet_markets_config
    if overlay is None:
        default_overlay = _DEFAULT_TESTNET_MARKETS_CONFIG.expanduser()
        overlay = default_overlay if default_overlay.exists() else None
    code = run_config_validate(
        program_config=program_path,
        markets_config=markets_path,
        testnet_markets_config=overlay,
    )
    if code != 0:
        raise SystemExit(code)


def default_cats_config_path() -> Path | None:
    home_candidate = Path("~/.greenfloor/config/cats.yaml").expanduser()
    if home_candidate.exists():
        return home_candidate
    repo_candidate = Path("config/cats.yaml")
    if repo_candidate.exists():
        return repo_candidate
    return None


def default_state_dir_path() -> Path:
    return Path("~/.greenfloor/state").expanduser()


def resolve_trade_asset_for_dexie(*, asset: str, network: str) -> str:
    normalized = asset.strip().lower()
    if normalized in {"xch", "txch", "1"}:
        return "txch" if is_testnet(network) else "xch"
    if is_hex_id(normalized):
        return normalized

    cats_path = default_cats_config_path()
    if cats_path is None:
        return asset
    try:
        raw = load_yaml(cats_path)
    except Exception:
        return asset
    if not isinstance(raw, dict):
        return asset
    cats = raw.get("cats", [])
    if not isinstance(cats, list):
        return asset
    for item in cats:
        if not isinstance(item, dict):
            continue
        symbol = str(item.get("base_symbol", "")).strip().lower()
        if symbol != normalized:
            continue
        asset_id = str(item.get("asset_id", "")).strip().lower()
        if is_hex_id(asset_id):
            return asset_id
    return asset


def resolve_quote_asset_for_offer(*, quote_asset: str, network: str) -> str:
    return resolve_trade_asset_for_dexie(asset=quote_asset, network=network)
