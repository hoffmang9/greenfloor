"""Subprocess bridge to ``greenfloor-engine kms-public-key-compressed-hex``."""

from __future__ import annotations

from greenfloor_scripts.engine_subprocess import (
    require_dict_payload,
    require_str_field,
    run_engine_json,
)


def get_public_key_compressed_hex(key_id: str, region: str) -> str:
    payload = require_dict_payload(
        run_engine_json(
            [
                "kms-public-key-compressed-hex",
                "--key-id",
                key_id,
                "--region",
                region,
            ]
        ),
        "kms_cli_invalid_response",
    )
    return require_str_field(payload, "public_key_compressed_hex", "kms_cli_missing_public_key")
