"""Resolved daemon config paths for Rust engine orchestration."""

from __future__ import annotations

from contextvars import ContextVar
from dataclasses import dataclass
from pathlib import Path

from greenfloor.config.models import ProgramConfig

_resolved_paths: ContextVar[ResolvedDaemonPaths | None] = ContextVar(
    "resolved_daemon_paths",
    default=None,
)


@dataclass(frozen=True, slots=True)
class ResolvedDaemonPaths:
    program_path: Path
    markets_path: Path
    testnet_markets_path: Path | None = None


def set_resolved_daemon_paths(paths: ResolvedDaemonPaths) -> None:
    _resolved_paths.set(paths)


def clear_resolved_daemon_paths() -> None:
    _resolved_paths.set(None)


def resolve_resolved_daemon_paths(
    program: ProgramConfig,
    program_path: Path | None = None,
) -> ResolvedDaemonPaths:
    explicit = _resolved_paths.get()
    if explicit is not None:
        return explicit
    if program_path is not None:
        resolved_program = program_path.expanduser().resolve()
        return ResolvedDaemonPaths(
            program_path=resolved_program,
            markets_path=resolved_program.parent / "markets.yaml",
        )
    home = Path(program.home_dir).expanduser()
    return ResolvedDaemonPaths(
        program_path=home / "config" / "program.yaml",
        markets_path=home / "config" / "markets.yaml",
    )
