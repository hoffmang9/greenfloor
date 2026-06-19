#!/usr/bin/env python3
"""CLI wrapper for vault Coinset coin scanning (delegates to greenfloor-engine)."""

from __future__ import annotations

import subprocess
import sys

from greenfloor_scripts.binaries import resolve_greenfloor_engine_binary


def main() -> int:
    binary = resolve_greenfloor_engine_binary(build_if_missing=False)
    result = subprocess.run([str(binary), "vault-coinset-scan", *sys.argv[1:]], check=False)
    return int(result.returncode)


if __name__ == "__main__":
    raise SystemExit(main())
