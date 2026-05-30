"""Invoke native greenfloor-engine offers lifecycle subcommands in tests."""

from __future__ import annotations

import subprocess
from pathlib import Path

from greenfloor.cli.engine_binary import resolve_greenfloor_engine_binary


def _run_engine(argv: list[str]) -> tuple[int, str, str]:
    binary = resolve_greenfloor_engine_binary()
    completed = subprocess.run(
        [str(binary), *argv],
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.stdout:
        print(completed.stdout, end="")
    if completed.stderr:
        import sys

        print(completed.stderr, end="", file=sys.stderr)
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
        "offers-reconcile",
        "--program-config",
        str(program_path),
        "--limit",
        str(int(limit)),
    ]
    if state_db:
        argv.extend(["--state-db", state_db])
    if market_id:
        argv.extend(["--market-id", market_id])
    if venue:
        argv.extend(["--venue", venue])
    code, _, _ = _run_engine(argv)
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
        "offers-status",
        "--program-config",
        str(program_path),
        "--limit",
        str(int(limit)),
        "--events-limit",
        str(int(events_limit)),
    ]
    if state_db:
        argv.extend(["--state-db", state_db])
    if market_id:
        argv.extend(["--market-id", market_id])
    code, _, _ = _run_engine(argv)
    return code


def offers_cancel(
    *,
    program_path: Path,
    offer_ids: list[str],
    cancel_open: bool,
    venue: str | None = None,
) -> int:
    argv = ["offers-cancel", "--program-config", str(program_path)]
    if cancel_open:
        argv.append("--cancel-open")
    for offer_id in offer_ids:
        argv.extend(["--offer-id", offer_id])
    if venue:
        argv.extend(["--venue", venue])
    code, _, _ = _run_engine(argv)
    return code
