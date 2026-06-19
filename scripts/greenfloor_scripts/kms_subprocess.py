"""Subprocess bridge to ``greenfloor-engine kms-public-key-compressed-hex``."""

from __future__ import annotations

from greenfloor_scripts.engine_subprocess import run_engine_json


def get_public_key_compressed_hex(key_id: str, region: str) -> str:
    payload = run_engine_json(
        [
            "kms-public-key-compressed-hex",
            "--key-id",
            key_id,
            "--region",
            region,
        ]
    )
    if not isinstance(payload, dict):
        raise RuntimeError("kms_cli_invalid_response")
    value = payload.get("public_key_compressed_hex")
    if not isinstance(value, str) or not value.strip():
        raise RuntimeError("kms_cli_missing_public_key")
    return value.strip()
