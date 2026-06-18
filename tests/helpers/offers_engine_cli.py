"""Invoke native greenfloor-manager offers lifecycle subcommands in tests."""

from __future__ import annotations

import subprocess
from pathlib import Path

from tests.helpers.manager_cli import resolve_greenfloor_manager_binary


def _run_manager(argv: list[str]) -> tuple[int, str, str]:
    binary = resolve_greenfloor_manager_binary()
    completed = subprocess.run(
        [str(binary), *argv],
        check=False,
        capture_output=True,
        text=True,
    )
    return int(completed.returncode), completed.stdout, completed.stderr


def offers_reconcile(
    *,
    program_path: Path,
    state_db: str | None,
    market_id: str | None,
    limit: int,
    venue: str | None,
) -> int:
    argv = [
        "--program-config",
        str(program_path),
        "offers-reconcile",
        "--limit",
        str(int(limit)),
    ]
    if state_db:
        argv.extend(["--state-db", state_db])
    if market_id:
        argv.extend(["--market-id", market_id])
    if venue:
        argv.extend(["--venue", venue])
    code, _, _ = _run_manager(argv)
    return code


def offers_status(
    *,
    program_path: Path,
    state_db: str | None,
    market_id: str | None,
    limit: int,
    events_limit: int,
) -> int:
    argv = [
        "--program-config",
        str(program_path),
        "offers-status",
        "--limit",
        str(int(limit)),
        "--events-limit",
        str(int(events_limit)),
    ]
    if state_db:
        argv.extend(["--state-db", state_db])
    if market_id:
        argv.extend(["--market-id", market_id])
    code, _, _ = _run_manager(argv)
    return code


def offers_cancel(
    *,
    program_path: Path,
    offer_ids: list[str],
    cancel_open: bool,
    venue: str | None = None,
) -> int:
    argv = ["--program-config", str(program_path), "offers-cancel"]
    if cancel_open:
        argv.append("--cancel-open")
    for offer_id in offer_ids:
        argv.extend(["--offer-id", offer_id])
    if venue:
        argv.extend(["--venue", venue])
    code, _, _ = _run_manager(argv)
    return code
