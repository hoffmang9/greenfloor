"""Coin-operation backends: factory and asset resolution."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters import rust_signer
from greenfloor.config.models import (
    MarketConfig,
    ProgramConfig,
    coin_ops_execution_backend,
    prepare_signer_runtime,
)
from greenfloor.hex_utils import canonical_is_xch
from greenfloor.runtime.cloud_wallet_coin_op_backend import CloudWalletCoinOpBackend
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
    "CloudWalletCoinOpBackend",
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
    deps: Any = None,
) -> str:
    """Resolve base asset id for coin-ops (signer coinset or Cloud Wallet catalog)."""
    from greenfloor.runtime.cloud_wallet.coin_ops_runtime import DEFAULT_COIN_OP_DEPS

    if coin_ops_execution_backend(program) == "signer":
        return resolve_signer_asset_id(
            program,
            canonical_asset_id=str(market.base_asset).strip(),
            symbol_hint=str(market.base_symbol).strip() or None,
        )
    coin_deps = deps if deps is not None else DEFAULT_COIN_OP_DEPS
    wallet = coin_deps.new_cloud_wallet_adapter(program)
    return coin_deps.resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=str(market.base_asset).strip(),
        symbol_hint=str(market.base_symbol).strip() or None,
        program_home_dir=str(program.home_dir),
    )


def build_coin_op_backend(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    selected_venue: str | None,
    resolved_asset_id: str,
    vault_id: str | None = None,
    deps: Any = None,
) -> CoinOpBackend:
    backend_name = coin_ops_execution_backend(program)
    receive_address = str(market.receive_address).strip()
    if backend_name == "signer":
        if not receive_address:
            raise ValueError("signer_coin_ops_missing_receive_address")
        return SignerCoinOpBackend(
            program=program,
            market=market,
            selected_venue=selected_venue,
            resolved_asset_id=resolved_asset_id,
            receive_address=receive_address,
        )
    from greenfloor.runtime.cloud_wallet.coin_ops_runtime import DEFAULT_COIN_OP_DEPS

    coin_deps = deps if deps is not None else DEFAULT_COIN_OP_DEPS
    wallet = coin_deps.new_cloud_wallet_adapter(program)
    if vault_id and vault_id.strip() and vault_id.strip() != wallet.vault_id:
        from greenfloor.runtime.cloud_wallet import adapter as cloud_wallet_adapter
        from greenfloor.runtime.cloud_wallet.adapter import (
            _require_cloud_wallet_config as require_cloud_wallet_config,
        )

        override_config = require_cloud_wallet_config(program)
        wallet = cloud_wallet_adapter.CloudWalletAdapter(
            cloud_wallet_adapter.CloudWalletConfig(
                base_url=override_config.base_url,
                user_key_id=override_config.user_key_id,
                private_key_pem_path=override_config.private_key_pem_path,
                vault_id=vault_id.strip(),
                network=override_config.network,
            )
        )
    return CloudWalletCoinOpBackend(
        program=program,
        market=market,
        wallet=wallet,
        selected_venue=selected_venue,
        resolved_asset_id=resolved_asset_id,
        deps=coin_deps,
    )
