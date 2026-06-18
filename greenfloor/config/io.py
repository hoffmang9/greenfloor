from __future__ import annotations

import subprocess
from pathlib import Path
from typing import Any

import yaml

from greenfloor.engine_binary import resolve_greenfloor_manager_binary
from tests.helpers.manager_cli import parse_json_output, run_manager

_MINIMAL_PROGRAM_TEMPLATE = (
    Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "data" / "minimal_program.yaml"
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


def is_testnet(network: str) -> bool:
    return network.strip().lower() in frozenset({"testnet", "testnet11"})


def materialize_minimal_program_template(
    path: Path,
    *,
    home_dir: Path,
    dexie_api_base: str = "https://api.dexie.space",
) -> None:
    text = _MINIMAL_PROGRAM_TEMPLATE.read_text(encoding="utf-8")
    text = text.replace("__HOME_DIR__", str(home_dir))
    text = text.replace("__DEXIE_API_BASE__", dexie_api_base)
    text = text.replace("__LOG_LEVEL__", "INFO")
    text = text.replace("__DRY_RUN__", "false")
    text = text.replace("__ALERTS_ENABLED__", "false")
    text = text.replace("__PUSHOVER_ENABLED__", "false")
    path.write_text(text, encoding="utf-8")


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


def enabled_market_rows(fields: dict[str, Any]) -> list[dict[str, Any]]:
    markets = fields.get("enabled_markets")
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
