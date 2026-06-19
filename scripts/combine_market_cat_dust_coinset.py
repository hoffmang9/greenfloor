#!/usr/bin/env python3
"""CLI wrapper for ``greenfloor-manager combine-market-cat-dust``."""

from __future__ import annotations

import subprocess
import sys

from greenfloor_scripts.binaries import resolve_greenfloor_manager_binary
from greenfloor_scripts.manager_subprocess import build_manager_argv


def main() -> int:
    binary = resolve_greenfloor_manager_binary(build_if_missing=False)
    result = subprocess.run(
        [str(binary), *build_manager_argv("combine-market-cat-dust", sys.argv[1:])],
        check=False,
    )
    return int(result.returncode)


if __name__ == "__main__":
    raise SystemExit(main())
