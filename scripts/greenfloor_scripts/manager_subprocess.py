"""Invoke native greenfloor-manager subcommands for scripts and tests."""

from __future__ import annotations

import json
import os
import subprocess
from functools import lru_cache

from greenfloor_scripts.binaries import resolve_greenfloor_manager_binary


@lru_cache(maxsize=32)
def _manager_flag_groups(subcommand: str) -> tuple[frozenset[str], frozenset[str], frozenset[str]]:
    binary = resolve_greenfloor_manager_binary(build_if_missing=False)
    completed = subprocess.run(
        [str(binary), "--json", "flag-groups", subcommand],
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"greenfloor-manager flag-groups failed for {subcommand}: {completed.stderr.strip()}"
        )
    payload = parse_json_output(completed.stdout)

    def _flags(key: str) -> frozenset[str]:
        section = payload.get(key, {})
        boolean = section.get("boolean", [])
        with_value = section.get("with_value", [])
        return frozenset(str(flag) for flag in [*boolean, *with_value])

    global_flags = _flags("global")
    subcommand_flags = _flags("subcommand_flags")
    flags_with_value = frozenset(
        str(flag)
        for flag in [
            *payload.get("global", {}).get("with_value", []),
            *payload.get("subcommand_flags", {}).get("with_value", []),
        ]
    )
    return global_flags, subcommand_flags, flags_with_value


def partition_manager_argv(subcommand: str, argv: list[str]) -> tuple[list[str], list[str]]:
    """Split argv into manager global flags and subcommand-specific args."""
    global_flags, subcommand_flags, flags_with_value = _manager_flag_groups(subcommand)
    global_args: list[str] = []
    subcommand_args: list[str] = []
    index = 0
    while index < len(argv):
        token = argv[index]
        if token.startswith("--"):
            flag = token.split("=", 1)[0]
            bucket = global_args if flag in global_flags else subcommand_args
            if flag not in global_flags and flag not in subcommand_flags:
                bucket = subcommand_args
            bucket.append(token)
            if "=" not in token and flag in flags_with_value:
                index += 1
                if index < len(argv):
                    bucket.append(argv[index])
            index += 1
            continue
        subcommand_args.extend(argv[index:])
        break
    return global_args, subcommand_args


def build_manager_argv(subcommand: str, argv: list[str]) -> list[str]:
    global_args, subcommand_args = partition_manager_argv(subcommand, argv)
    return [*global_args, subcommand, *subcommand_args]


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
