"""Invoke native greenfloor-manager subcommands in tests."""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path

from tests.helpers.engine_binary import resolve_greenfloor_engine_binary


def resolve_greenfloor_manager_binary() -> Path:
    manager = resolve_greenfloor_engine_binary().with_name("greenfloor-manager")
    if manager.exists():
        return manager
    raise FileNotFoundError(
        "greenfloor-manager binary not found; build with "
        "`cargo build --manifest-path greenfloor-engine/Cargo.toml --bin greenfloor-manager`"
    )


def run_manager(
    argv: list[str],
    *,
    stdin: str | None = None,
    env: dict[str, str] | None = None,
) -> tuple[int, str, str]:
    binary = resolve_greenfloor_manager_binary()
    run_env = os.environ.copy()
    if env:
        run_env.update(env)
    completed = subprocess.run(
        [str(binary), *argv],
        check=False,
        capture_output=True,
        text=True,
        input=stdin,
        env=run_env,
    )
    return int(completed.returncode), completed.stdout, completed.stderr


def parse_json_output(stdout: str) -> dict:
    text = stdout.strip()
    if not text:
        return {}
    start = text.find("{")
    if start == -1:
        return json.loads(text)
    return json.loads(text[start:])
