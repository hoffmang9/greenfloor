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


def resolve_client_cli(network: str, base_url: str | None) -> tuple[str, str]:
    payload = run_engine_json(["coinset", "resolve-client", *_client_flags(network, base_url)])
    if not isinstance(payload, dict):
        raise RuntimeError("coinset_resolve_client_invalid_payload")
    resolved_network = str(payload.get("network") or "").strip()
    resolved_base_url = str(payload.get("base_url") or "").strip()
    if not resolved_network or not resolved_base_url:
        raise RuntimeError("coinset_resolve_client_missing_fields")
    return resolved_network, resolved_base_url


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
    argv = [
        "coinset",
        "coin-records",
        *_client_flags(network, base_url),
        "--endpoint",
        endpoint,
        "--body-json",
        json.dumps(payload_body, separators=(",", ":")),
    ]
    payload = run_engine_json(argv)
    if not isinstance(payload, dict):
        raise RuntimeError("coinset_coin_records_invalid_payload")
    records = payload.get("coin_records") or []
    return [record for record in records if isinstance(record, dict)]


def record_from_cli(
    network: str,
    base_url: str | None,
    endpoint: str,
    body: dict[str, Any],
    key: str,
) -> dict[str, Any] | None:
    argv = [
        "coinset",
        "record",
        *_client_flags(network, base_url),
        "--endpoint",
        endpoint,
        "--body-json",
        json.dumps(dict(body), separators=(",", ":")),
        "--key",
        key,
    ]
    payload = run_engine_json(argv)
    if not isinstance(payload, dict):
        raise RuntimeError("coinset_record_invalid_payload")
    record = payload.get("record")
    return record if isinstance(record, dict) else None
