"""Coin-operation backends: signer-only factory and asset resolution."""

from __future__ import annotations

from greenfloor.adapters import rust_signer
from greenfloor.config.models import MarketConfig, ProgramConfig, prepare_signer_runtime
from greenfloor.hex_utils import canonical_is_xch
from greenfloor.runtime.coin_ops_scope import (
    CoinOpBackend,
    CoinOpExecutionBackend,
    CoinOpScope,
    scope_payload,
)
from greenfloor.runtime.signer_coin_op_backend import SignerCoinOpBackend

__all__ = [
    "CoinOpBackend",
    "CoinOpExecutionBackend",
    "CoinOpScope",
    "SignerCoinOpBackend",
    "build_coin_op_backend",
    "resolve_coin_op_base_asset_id",
    "resolve_signer_asset_id",
    "scope_payload",
]


def resolve_signer_asset_id(
    program: ProgramConfig,
    *,
    canonical_asset_id: str,
    symbol_hint: str | None = None,
) -> str:
    _ = symbol_hint
    base = str(canonical_asset_id).strip()
    if canonical_is_xch(base):
        return "xch"
    config_path = prepare_signer_runtime(program)
    resolved = rust_signer.resolve_offer_asset_ids(config_path, base, "xch")
    return str(resolved["base_asset_id"]).strip().lower()


def resolve_coin_op_base_asset_id(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    deps: object = None,
) -> str:
    _ = deps
    return resolve_signer_asset_id(
        program,
        canonical_asset_id=str(market.base_asset).strip(),
        symbol_hint=str(market.base_symbol).strip() or None,
    )


def build_coin_op_backend(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    selected_venue: str | None,
    resolved_asset_id: str,
    vault_id: str | None = None,
    deps: object = None,
) -> CoinOpBackend:
    _ = vault_id, deps
    receive_address = str(market.receive_address).strip()
    if not receive_address:
        raise ValueError("signer_coin_ops_missing_receive_address")
    return SignerCoinOpBackend(
        program=program,
        market=market,
        selected_venue=selected_venue,
        resolved_asset_id=resolved_asset_id,
        receive_address=receive_address,
    )
