"""Run native greenfloor-engine CLI subcommands."""

from __future__ import annotations

import subprocess
from collections.abc import Sequence

from greenfloor.cli.engine_binary import resolve_greenfloor_engine_binary


def run_engine_cli(argv: Sequence[str]) -> int:
    binary = resolve_greenfloor_engine_binary()
    completed = subprocess.run([str(binary), *argv], check=False)
    return int(completed.returncode)
