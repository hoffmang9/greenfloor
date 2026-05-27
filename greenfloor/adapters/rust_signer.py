"""Thin wrapper around the ``greenfloor_signer`` PyO3 extension."""

from __future__ import annotations

import importlib
from pathlib import Path
from typing import Any

_SIGNER_MODULE = "greenfloor_signer"
_INSTALL_HINT = (
    "Install the greenfloor_signer extension (for example: "
    "`maturin develop -m greenfloor-signer-pyo3` from the repo root)."
)


def _import_greenfloor_signer() -> Any:
    try:
        return importlib.import_module(_SIGNER_MODULE)
    except ImportError as exc:
        raise ImportError(
            f"{_SIGNER_MODULE} is not available. {_INSTALL_HINT} Original error: {exc}"
        ) from exc


def resolve_vault_context(program_path: str) -> dict[str, Any]:
    """Load vault display context from program config via the Rust signer."""
    signer = _import_greenfloor_signer()
    result = signer.resolve_vault_context(str(program_path))
    if not isinstance(result, dict):
        raise TypeError("resolve_vault_context returned non-dict result")
    return result


def build_vault_cat_offer(program_path: str, request_dict: dict[str, Any]) -> dict[str, Any]:
    """Build a vault CAT offer using the canonical Rust signer."""
    signer = _import_greenfloor_signer()
    result = signer.build_vault_cat_offer(str(program_path), request_dict)
    if not isinstance(result, dict):
        raise TypeError("build_vault_cat_offer returned non-dict result")
    return result


def build_mixed_split(program_path: str, request_dict: dict[str, Any]) -> dict[str, Any]:
    """Build (and optionally broadcast) a vault CAT mixed split via the Rust signer."""
    signer = _import_greenfloor_signer()
    result = signer.build_mixed_split(str(program_path), request_dict)
    if not isinstance(result, dict):
        raise TypeError("build_mixed_split returned non-dict result")
    return result


def resolve_offer_asset_ids(program_path: str, base_asset: str, quote_asset: str) -> dict[str, str]:
    """Resolve market symbols or asset ids to canonical offer asset ids."""
    signer = _import_greenfloor_signer()
    result = signer.resolve_offer_asset_ids(str(program_path), base_asset, quote_asset)
    if not isinstance(result, dict):
        raise TypeError("resolve_offer_asset_ids returned non-dict result")
    base_asset_id = str(result.get("base_asset_id", "")).strip()
    quote_asset_id = str(result.get("quote_asset_id", "")).strip()
    if not base_asset_id or not quote_asset_id:
        raise ValueError("resolve_offer_asset_ids_missing_fields")
    return {"base_asset_id": base_asset_id, "quote_asset_id": quote_asset_id}


def program_config_path_from_payload(payload: dict[str, Any]) -> str | None:
    """Resolve program.yaml path from signing/offer payload fields."""
    for key in ("program_config_path", "program_config", "program_path"):
        value = str(payload.get(key, "")).strip()
        if value:
            return value
    home = str(payload.get("program_home_dir", "")).strip()
    if home:
        return str(Path(home).expanduser() / "config" / "program.yaml")
    return None
