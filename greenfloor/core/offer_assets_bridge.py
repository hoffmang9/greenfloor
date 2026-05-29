"""Rust-backed offer asset resolution (canonical Python bridge)."""

from __future__ import annotations

from typing import TYPE_CHECKING

from greenfloor.core import engine_bridge

if TYPE_CHECKING:
    from greenfloor.config.models import ProgramConfig

__all__ = [
    "resolve_offer_asset_ids_by_config_path",
    "resolve_offer_asset_ids_for_program",
    "resolve_offer_assets",
    "try_normalize_offer_asset_ids",
]

_require_offer_action_method = engine_bridge.engine_method_getter(
    lambda: engine_bridge.policy_engine(),
    missing="offer-action",
)

_EMPTY_RESOLVED_ASSET_ERROR = "signer_asset_resolution_failed:empty_resolved_asset_id"


def _coerce_asset_pair(result: object) -> tuple[str, str]:
    if not isinstance(result, tuple) or len(result) != 2:
        raise TypeError("offer asset resolution returned non-pair result")
    base_asset_id = str(result[0]).strip()
    quote_asset_id = str(result[1]).strip()
    if not base_asset_id or not quote_asset_id:
        raise RuntimeError(_EMPTY_RESOLVED_ASSET_ERROR)
    return base_asset_id, quote_asset_id


def try_normalize_offer_asset_ids(base_asset: str, quote_asset: str) -> tuple[str, str] | None:
    """Normalize a base/quote pair when both inputs are already canonical.

    Returns ``None`` when either asset is not directly normalizable (for example a
    market ticker symbol). That ``None`` means "fall back to Coinset-backed
    resolution", not success with empty values. Raises when normalization succeeds
    structurally but the pair collides for a non-XCH market.
    """
    normalize = _require_offer_action_method("try_normalize_offer_asset_ids")
    payload = normalize(str(base_asset).strip(), str(quote_asset).strip())
    if payload is None:
        return None
    return _coerce_asset_pair(payload)


def resolve_offer_asset_ids_by_config_path(
    config_path: str,
    base_asset: str,
    quote_asset: str,
) -> tuple[str, str]:
    """Resolve offer assets via signer config path and Coinset (no normalize retry)."""
    resolve = _require_offer_action_method("resolve_offer_asset_ids")
    payload = resolve(str(config_path), str(base_asset).strip(), str(quote_asset).strip())
    return _coerce_asset_pair(payload)


def resolve_offer_asset_ids_for_program(
    program: ProgramConfig,
    base_asset: str,
    quote_asset: str,
) -> tuple[str, str]:
    """Resolve offer assets for a program config (writes signer.yaml when needed)."""
    from greenfloor.config.models import prepare_signer_runtime

    config_path = prepare_signer_runtime(program)
    return resolve_offer_asset_ids_by_config_path(config_path, base_asset, quote_asset)


def resolve_offer_assets(
    base_asset: str,
    quote_asset: str,
    *,
    program: ProgramConfig,
) -> tuple[str, str]:
    """Resolve market symbols or asset ids to canonical offer asset ids."""
    base = str(base_asset).strip()
    quote = str(quote_asset).strip()
    normalized = try_normalize_offer_asset_ids(base, quote)
    if normalized is not None:
        return normalized
    return resolve_offer_asset_ids_for_program(program, base, quote)
