"""Daemon config path resolution for Rust engine orchestration."""

from __future__ import annotations

from contextvars import ContextVar
from dataclasses import dataclass
from pathlib import Path

from greenfloor.config.models import ProgramConfig

_config_paths: ContextVar[DaemonConfigPaths | None] = ContextVar(
    "daemon_config_paths",
    default=None,
)


@dataclass(frozen=True, slots=True)
class DaemonConfigPaths:
    program_path: Path
    markets_path: Path
    testnet_markets_path: Path | None = None


def set_daemon_config_paths(paths: DaemonConfigPaths) -> None:
    _config_paths.set(paths)


def clear_daemon_config_paths() -> None:
    _config_paths.set(None)


def resolve_daemon_config_paths(
    program: ProgramConfig,
    program_path: Path | None = None,
) -> DaemonConfigPaths:
    explicit = _config_paths.get()
    if explicit is not None:
        return explicit
    if program_path is not None:
        resolved_program = program_path.expanduser().resolve()
        return DaemonConfigPaths(
            program_path=resolved_program,
            markets_path=resolved_program.parent / "markets.yaml",
        )
    home = Path(program.home_dir).expanduser()
    return DaemonConfigPaths(
        program_path=home / "config" / "program.yaml",
        markets_path=home / "config" / "markets.yaml",
    )
