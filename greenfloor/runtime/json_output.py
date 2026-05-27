"""Shared JSON formatting for manager CLI output."""

from __future__ import annotations

import json

_JSON_OUTPUT_COMPACT = False


def set_json_output_compact(compact: bool) -> None:
    global _JSON_OUTPUT_COMPACT
    _JSON_OUTPUT_COMPACT = bool(compact)


def format_json_output(payload: object) -> str:
    if _JSON_OUTPUT_COMPACT:
        return json.dumps(payload, separators=(",", ":"))
    return json.dumps(payload, indent=2)
