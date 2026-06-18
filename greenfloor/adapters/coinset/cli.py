"""Subprocess bridge to ``greenfloor-engine coinset`` CLI."""

from __future__ import annotations

import json
import subprocess
from typing import Any

from greenfloor.engine_binary import (
    GreenfloorEngineBinaryError,
    resolve_greenfloor_engine_binary,
)


def run_engine_json(argv: list[str]) -> Any:
    try:
        binary = resolve_greenfloor_engine_binary(build_if_missing=False)
    except GreenfloorEngineBinaryError as exc:
        raise RuntimeError(f"coinset_cli_binary_unavailable: {exc}") from exc
    cmd = [str(binary), *argv, "--json"]
    result = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "").strip()
        raise RuntimeError(f"coinset_cli_failed:{detail}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError("coinset_cli_invalid_json") from exc


def run_coinset_cli(subcommand: str, flags: list[tuple[str, str]]) -> Any:
    argv = ["coinset", subcommand]
    for flag, value in flags:
        argv.extend([flag, value])
    return run_engine_json(argv)


def post_json_cli(
    network: str,
    base_url: str,
    endpoint: str,
    body: dict[str, Any],
) -> Any:
    return run_coinset_cli(
        "post",
        [
            ("--network", network),
            ("--base-url", base_url),
            ("--endpoint", endpoint),
            ("--body-json", json.dumps(body, separators=(",", ":"))),
        ],
    )


def push_tx_cli(network: str, base_url: str, spend_bundle_hex: str) -> Any:
    return run_coinset_cli(
        "push-tx",
        [
            ("--network", network),
            ("--base-url", base_url),
            ("--spend-bundle-hex", spend_bundle_hex),
        ],
    )
