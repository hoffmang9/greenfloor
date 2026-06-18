"""Shared helpers for mocking greenfloor-engine coinset CLI calls in tests."""

from __future__ import annotations

import json
from collections.abc import Callable
from typing import Any


def parse_coinset_cli_flags(argv: list[str]) -> dict[str, str]:
    flags: dict[str, str] = {}
    index = 0
    while index < len(argv):
        token = argv[index]
        if token.startswith("--") and index + 1 < len(argv):
            flags[token[2:]] = argv[index + 1]
            index += 2
            continue
        index += 1
    return flags


def default_coinset_cli_handler(argv: list[str]) -> Any:
    if len(argv) < 2 or argv[0] != "coinset":
        raise AssertionError(f"unexpected_coinset_cli_argv:{argv}")
    subcommand = argv[1]
    if subcommand == "push-tx":
        return {"success": True, "status": "submitted"}
    if subcommand == "post":
        flags = parse_coinset_cli_flags(argv[2:])
        endpoint = flags.get("endpoint", "")
        if endpoint == "get_all_mempool_tx_ids":
            return {"success": True, "mempool_tx_ids": ["0xabc", "0xdef"]}
        if endpoint == "get_fee_estimate":
            return {"success": True, "estimates": [100, 500, 200]}
        raise AssertionError(f"unexpected_coinset_post_endpoint:{endpoint}")
    raise AssertionError(f"unexpected_coinset_subcommand:{subcommand}")


def make_coinset_cli_handler(
    *,
    post_handler: Callable[[str, dict[str, Any]], Any] | None = None,
) -> Callable[[list[str]], Any]:
    def _handler(argv: list[str]) -> Any:
        if len(argv) >= 2 and argv[0] == "coinset" and argv[1] == "post" and post_handler:
            flags = parse_coinset_cli_flags(argv[2:])
            endpoint = flags.get("endpoint", "")
            body_raw = flags.get("body-json", "{}")
            body = json.loads(body_raw)
            if not isinstance(body, dict):
                raise AssertionError(f"invalid_post_body:{body_raw}")
            return post_handler(endpoint, body)
        return default_coinset_cli_handler(argv)

    return _handler
