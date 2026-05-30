"""Native greenfloor-engine daemon entry (replaces in-process Python orchestration)."""

from __future__ import annotations

import sys

from greenfloor.cli.engine_subprocess import run_engine_cli


def main() -> None:
    raise SystemExit(run_engine_cli(["daemon", *sys.argv[1:]]))
