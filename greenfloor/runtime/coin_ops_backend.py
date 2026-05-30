"""Coin-operation backends: signer-only factory and asset resolution."""

from __future__ import annotations

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.offer_assets_bridge import resolve_offer_assets
from greenfloor.runtime.coin_ops_scope import CoinOpBackend, CoinOpScope, scope_payload
from greenfloor.runtime.signer_coin_op_backend import SignerCoinOpBackend

__all__ = [
    "CoinOpBackend",
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
) -> str:
    base, _quote = resolve_offer_assets(canonical_asset_id, "xch", program=program)
    return base.lower()


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
