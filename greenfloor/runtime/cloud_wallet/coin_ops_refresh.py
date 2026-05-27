"""On-chain refresh split after off-chain offer cancel."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.offer_decode import extract_coin_id_hints_from_offer_text
from greenfloor.runtime.cloud_wallet.coin_ops_runtime import CoinOpDeps, DEFAULT_COIN_OP_DEPS
from greenfloor.runtime.cloud_wallet.coins import is_spendable_coin, resolve_coin_global_ids


def execute_offer_onchain_refresh_split(
    *,
    wallet: CloudWalletAdapter,
    market: MarketConfig,
    program: ProgramConfig,
    offer_bech32: str,
    deps: CoinOpDeps = DEFAULT_COIN_OP_DEPS,
) -> dict[str, Any]:
    """Submit a no-op split to refresh on-chain coin state after off-chain cancel."""
    resolved_asset_id = deps.resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=market.base_asset,
        symbol_hint=market.base_symbol,
        program_home_dir=str(program.home_dir),
    )
    market_coins = wallet.list_coins(
        asset_id=resolved_asset_id,
        include_pending=True,
    )
    spendable_market_coins = [coin for coin in market_coins if is_spendable_coin(coin)]
    if not spendable_market_coins:
        raise RuntimeError("no_spendable_market_coins_for_onchain_refresh")
    coin_id_hints = extract_coin_id_hints_from_offer_text(str(offer_bech32).strip())
    resolved_coin_ids, _ = resolve_coin_global_ids(spendable_market_coins, coin_id_hints)
    target_coin: dict[str, Any] | None = None
    if resolved_coin_ids:
        for coin in spendable_market_coins:
            if str(coin.get("id", "")).strip() == resolved_coin_ids[0]:
                target_coin = coin
                break
    if target_coin is None:
        target_coin = sorted(
            spendable_market_coins,
            key=lambda c: int(c.get("amount", 0)),
        )[0]
    refresh_fee_mojos, refresh_fee_source = deps.resolve_taker_or_coin_operation_fee(
        network=program.app_network,
        minimum_fee_mojos=0,
    )
    refresh_result = wallet.split_coins(
        coin_ids=[str(target_coin.get("id", "")).strip()],
        amount_per_coin=int(target_coin.get("amount", 0)),
        number_of_coins=1,
        fee=int(refresh_fee_mojos),
    )
    refresh_signature_request_id = str(refresh_result.get("signature_request_id", "")).strip()
    return {
        "status": ("executed" if refresh_signature_request_id else "skipped"),
        "reason": (
            "cloud_wallet_split_submitted"
            if refresh_signature_request_id
            else "missing_signature_request_id"
        ),
        "signature_request_id": refresh_signature_request_id or None,
        "signature_state": str(refresh_result.get("status", "")).strip(),
        "coin_id": str(target_coin.get("id", "")).strip(),
        "coin_name": str(target_coin.get("name", "")).strip(),
        "amount": int(target_coin.get("amount", 0)),
        "asset_id": resolved_asset_id,
        "fee_mojos": int(refresh_fee_mojos),
        "fee_source": refresh_fee_source,
    }
