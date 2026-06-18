from __future__ import annotations

import subprocess
from pathlib import Path
from typing import Any

import yaml

from greenfloor.engine_binary import resolve_greenfloor_manager_binary
from greenfloor.hex_utils import normalize_hex_id
from tests.helpers.manager_cli import parse_json_output, run_manager


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
    return network.strip().lower() in frozenset({"testnet", "testnet11"})


def materialize_minimal_program_template(
    path: Path,
    *,
    home_dir: Path,
    dexie_api_base: str = "https://api.dexie.space",
    log_level: str = "INFO",
    dry_run: bool = False,
    low_inventory_alerts_enabled: bool = False,
    pushover_enabled: bool = False,
    with_signer: bool = False,
) -> None:
    """Materialize the shared minimal program template via native Rust policy."""
    path.parent.mkdir(parents=True, exist_ok=True)
    argv = [
        str(resolve_greenfloor_manager_binary()),
        "materialize-minimal-program",
        "--output",
        str(path),
        "--home-dir",
        str(home_dir),
        "--dexie-api-base",
        dexie_api_base,
        "--log-level",
        log_level,
    ]
    if dry_run:
        argv.append("--dry-run")
    if low_inventory_alerts_enabled:
        argv.append("--low-inventory-alerts-enabled")
    if pushover_enabled:
        argv.append("--pushover-enabled")
    if with_signer:
        argv.append("--with-signer")
    completed = subprocess.run(argv, check=False)
    if completed.returncode != 0:
        raise RuntimeError(f"materialize-minimal-program failed with exit {completed.returncode}")


def run_config_validate(
    *,
    program_config: Path,
    markets_config: Path | None = None,
    testnet_markets_config: Path | None = None,
    program_only: bool = False,
) -> int:
    """Validate operator config via native ``greenfloor-manager config-validate``."""
    argv = [
        str(resolve_greenfloor_manager_binary()),
        "--program-config",
        str(program_config),
    ]
    if program_only:
        argv.extend(["config-validate", "--program-only"])
    else:
        if markets_config is None:
            raise ValueError("markets_config is required unless program_only=True")
        argv.extend(
            [
                "--markets-config",
                str(markets_config),
            ]
        )
        if testnet_markets_config is not None:
            argv.extend(
                [
                    "--testnet-markets-config",
                    str(testnet_markets_config),
                ]
            )
        argv.append("config-validate")
    completed = subprocess.run(argv, check=False)
    return int(completed.returncode)


def load_program_fields(*, program_config: Path) -> dict[str, Any]:
    """Load script-facing program fields via native ``greenfloor-manager program-fields``."""
    code, stdout, stderr = run_manager(
        [
            "--program-config",
            str(program_config),
            "--json",
            "program-fields",
        ]
    )
    if code != 0:
        detail = stderr.strip() or stdout.strip() or f"exit {code}"
        raise RuntimeError(f"program-fields failed: {detail}")
    payload = parse_json_output(stdout)
    if not isinstance(payload, dict):
        raise RuntimeError("program-fields returned non-object JSON")
    return payload


def load_markets_fields(
    *,
    markets_config: Path,
    testnet_markets_config: Path | None = None,
) -> dict[str, Any]:
    """Load script-facing enabled markets via native ``greenfloor-manager markets-fields``."""
    argv = [
        "--markets-config",
        str(markets_config),
    ]
    if testnet_markets_config is not None:
        argv.extend(["--testnet-markets-config", str(testnet_markets_config)])
    argv.extend(["--json", "markets-fields"])
    code, stdout, stderr = run_manager(argv)
    if code != 0:
        detail = stderr.strip() or stdout.strip() or f"exit {code}"
        raise RuntimeError(f"markets-fields failed: {detail}")
    payload = parse_json_output(stdout)
    if not isinstance(payload, dict):
        raise RuntimeError("markets-fields returned non-object JSON")
    return payload


def load_cats_fields(*, cats_config: Path) -> dict[str, Any]:
    """Load script-facing CAT catalog fields via native ``greenfloor-manager cats-fields``."""
    code, stdout, stderr = run_manager(
        [
            "--cats-config",
            str(cats_config),
            "--json",
            "cats-fields",
        ]
    )
    if code != 0:
        detail = stderr.strip() or stdout.strip() or f"exit {code}"
        raise RuntimeError(f"cats-fields failed: {detail}")
    payload = parse_json_output(stdout)
    if not isinstance(payload, dict):
        raise RuntimeError("cats-fields returned non-object JSON")
    return payload


def symbol_to_asset_id_map(fields: dict[str, Any]) -> dict[str, str]:
    raw = fields.get("symbol_to_asset_id")
    if not isinstance(raw, dict):
        return {}
    out: dict[str, str] = {}
    for symbol, asset_id in raw.items():
        normalized = normalize_hex_id(str(asset_id))
        if normalized:
            out[str(symbol).strip().lower()] = normalized
    return out


def enabled_market_rows(fields: dict[str, Any]) -> list[dict[str, Any]]:
    markets = fields.get("enabled_markets")
    if not isinstance(markets, list):
        return []
    return [row for row in markets if isinstance(row, dict)]


def all_market_rows(fields: dict[str, Any]) -> list[dict[str, Any]]:
    markets = fields.get("markets")
    if not isinstance(markets, list):
        return []
    return [row for row in markets if isinstance(row, dict)]


def ensure_program_config_valid(*, program_config: Path | None = None) -> None:
    """Run native program-only validation using the default path when omitted."""
    program_path = (program_config or Path("~/.greenfloor/config/program.yaml")).expanduser()
    code = run_config_validate(program_config=program_path, program_only=True)
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
