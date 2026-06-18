"""Test-only Coinset push fallback using structured spend bundles via Rust CLI."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.coinset_cli_mutate import post_json_cli


def push_tx_structured(
    *,
    network: str,
    base_url: str,
    spend_bundle: dict[str, Any],
) -> dict[str, Any]:
    payload = post_json_cli(network, base_url, "push_tx", {"spend_bundle": spend_bundle})
    if not isinstance(payload, dict):
        return {"success": False, "error": "invalid_response_payload"}
    return payload
