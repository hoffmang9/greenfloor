"""Subprocess bridge to ``greenfloor-engine hex`` for script hex normalization."""

from __future__ import annotations

import json

from greenfloor_scripts.engine_subprocess import (
    require_dict_payload,
    require_int_field,
    require_list_field,
    run_engine_json,
)


class HexNormalizer:
    """Batch hex normalization backed by ``greenfloor-engine hex normalize-batch``.

    Module-level cache avoids repeated subprocess calls during vault scans.
    """

    def __init__(self) -> None:
        self._cache: dict[str, str] = {}

    def normalize(self, value: object) -> str:
        if not isinstance(value, str):
            return ""
        if value in self._cache:
            return self._cache[value]
        self._fetch_missing([value])
        return self._cache.get(value, "")

    def normalize_many(self, values: list[str]) -> list[str]:
        missing = [value for value in values if value not in self._cache]
        if missing:
            self._fetch_missing(missing)
        return [self._cache.get(value, "") for value in values]

    def _fetch_missing(self, values: list[str]) -> None:
        unique = list(dict.fromkeys(values))
        if not unique:
            return
        payload = require_dict_payload(
            run_engine_json(
                [
                    "hex",
                    "normalize-batch",
                    "--values-json",
                    json.dumps(unique, separators=(",", ":")),
                ]
            ),
            "hex_cli_invalid_response",
        )
        normalized = require_list_field(
            payload,
            "normalized",
            "hex_cli_invalid_normalized_batch",
        )
        if len(normalized) != len(unique):
            raise RuntimeError("hex_cli_invalid_normalized_batch")
        for raw, normalized_value in zip(unique, normalized, strict=True):
            self._cache[raw] = str(normalized_value)


_default_normalizer = HexNormalizer()


def normalize_hex_id(value: object) -> str:
    return _default_normalizer.normalize(value)


def normalize_hex_ids(values: list[str]) -> list[str]:
    return _default_normalizer.normalize_many(values)


def is_hex_id(value: str) -> bool:
    payload = require_dict_payload(
        run_engine_json(["hex", "is-id", "--value", str(value)]),
        "hex_cli_invalid_response",
    )
    return bool(payload.get("is_hex_id"))


def default_mojo_multiplier_for_asset(asset_id: str) -> int:
    payload = require_dict_payload(
        run_engine_json(["hex", "default-mojo-multiplier", "--asset-id", asset_id]),
        "hex_cli_invalid_response",
    )
    return require_int_field(payload, "multiplier", "hex_cli_missing_multiplier")
