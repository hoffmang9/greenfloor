"""Subprocess bridge to ``greenfloor-engine kms-public-key-compressed-hex``."""

from __future__ import annotations

import json
import subprocess

from greenfloor_scripts.binaries import (
    GreenfloorEngineBinaryError,
    resolve_greenfloor_engine_binary,
)


def get_public_key_compressed_hex(key_id: str, region: str) -> str:
    try:
        binary = resolve_greenfloor_engine_binary(build_if_missing=False)
    except GreenfloorEngineBinaryError as exc:
        raise RuntimeError(f"kms_cli_binary_unavailable: {exc}") from exc
    completed = subprocess.run(
        [
            str(binary),
            "kms-public-key-compressed-hex",
            "--key-id",
            key_id,
            "--region",
            region,
            "--json",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout or "").strip()
        raise RuntimeError(f"kms_cli_failed:{detail}")
    payload = json.loads(completed.stdout)
    if not isinstance(payload, dict):
        raise RuntimeError("kms_cli_invalid_response")
    value = payload.get("public_key_compressed_hex")
    if not isinstance(value, str) or not value.strip():
        raise RuntimeError("kms_cli_missing_public_key")
    return value.strip()
