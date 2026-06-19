"""Subprocess bridge to ``greenfloor-engine coinset`` for Python scripts."""

from __future__ import annotations

import json
from typing import Any

from greenfloor_scripts.engine_subprocess import run_engine_json


def _client_flags(network: str, base_url: str | None) -> list[str]:
    flags = ["--network", network.strip()]
    if base_url:
        flags.extend(["--base-url", base_url.strip()])
    return flags


def apply_height_fields(
    body: dict[str, Any],
    *,
    start_height: int | None,
    end_height: int | None,
) -> None:
    if start_height is not None:
        body["start_height"] = int(start_height)
    if end_height is not None:
        body["end_height"] = int(end_height)


def coin_records_from_payload(payload: dict[str, Any]) -> list[dict[str, Any]]:
    if not payload.get("success"):
        return []
    records = payload.get("coin_records") or []
    return [record for record in records if isinstance(record, dict)]


def record_from_payload(payload: dict[str, Any], key: str) -> dict[str, Any] | None:
    if not payload.get("success"):
        return None
    record = payload.get(key)
    return record if isinstance(record, dict) else None


def post_json_cli(
    network: str,
    base_url: str | None,
    endpoint: str,
    body: dict[str, Any],
) -> dict[str, Any]:
    argv = [
        "coinset",
        "post",
        *_client_flags(network, base_url),
        "--endpoint",
        endpoint,
        "--body-json",
        json.dumps(body, separators=(",", ":")),
    ]
    payload = run_engine_json(argv)
    if not isinstance(payload, dict):
        raise RuntimeError("coinset_invalid_response_payload")
    return payload


def push_tx_cli(network: str, base_url: str | None, spend_bundle_hex: str) -> dict[str, Any]:
    argv = [
        "coinset",
        "push-tx",
        *_client_flags(network, base_url),
        "--spend-bundle-hex",
        spend_bundle_hex,
    ]
    payload = run_engine_json(argv)
    if not isinstance(payload, dict):
        raise RuntimeError("coinset_push_tx_invalid_response")
    return payload


def coin_records_cli(
    network: str,
    base_url: str | None,
    endpoint: str,
    body: dict[str, Any],
    *,
    start_height: int | None = None,
    end_height: int | None = None,
) -> list[dict[str, Any]]:
    payload_body = dict(body)
    apply_height_fields(payload_body, start_height=start_height, end_height=end_height)
    return coin_records_from_payload(post_json_cli(network, base_url, endpoint, payload_body))


def record_from_cli(
    network: str,
    base_url: str | None,
    endpoint: str,
    body: dict[str, Any],
    key: str,
) -> dict[str, Any] | None:
    return record_from_payload(
        post_json_cli(network, base_url, endpoint, dict(body)),
        key,
    )
