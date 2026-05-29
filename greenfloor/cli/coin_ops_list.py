"""CLI coin listing commands (signer + coinset backend)."""

from __future__ import annotations

import logging
import sys
from pathlib import Path
from typing import Any

from greenfloor.config.io import load_markets_config, load_program_config
from greenfloor.config.models import ProgramConfig, coin_ops_execution_backend
from greenfloor.runtime.coin_ops.coins import is_spendable_coin
from greenfloor.runtime.coin_ops_backend import build_coin_op_backend, resolve_signer_asset_id
from greenfloor.runtime.json_output import format_json_output

coin_ops_logger = logging.getLogger("greenfloor.manager")


def _resolve_coins_list_market(program: ProgramConfig) -> Any:
    markets_path = Path(program.home_dir) / "config" / "markets.yaml"
    markets = load_markets_config(markets_path)
    enabled = [market for market in markets.markets if market.enabled]
    candidates = enabled or list(markets.markets)
    if not candidates:
        raise ValueError("no markets configured")
    if len(candidates) == 1:
        return candidates[0]
    return min(candidates, key=lambda market: str(market.market_id))


def _coins_list_signer(
    *,
    program: Any,
    asset: str | None,
    cat_id: str | None,
) -> int:
    assert isinstance(program, ProgramConfig)
    market = _resolve_coins_list_market(program)
    receive_address = str(market.receive_address).strip()
    if not receive_address:
        raise ValueError("market missing receive_address for signer coin list")

    canonical_filter = None
    if cat_id and cat_id.strip():
        canonical_filter = cat_id.strip().lower()
    elif asset and asset.strip():
        canonical_filter = asset.strip()
    resolved_asset_filter: str | None = None
    if canonical_filter:
        resolved_asset_filter = resolve_signer_asset_id(
            program, canonical_asset_id=canonical_filter
        )
    list_asset_id = resolved_asset_filter or str(market.base_asset)
    backend = build_coin_op_backend(
        program=program,
        market=market,
        selected_venue=None,
        resolved_asset_id=list_asset_id,
    )
    coins = backend.list_asset_scoped_coins()
    items = []
    for coin in coins:
        coin_state = str(coin.get("state", "CONFIRMED")).strip().upper()
        items.append(
            {
                "coin_id": str(coin.get("name", coin.get("id", ""))).strip(),
                "amount": int(coin.get("amount", 0)),
                "state": coin_state,
                "pending": coin_state in {"PENDING", "MEMPOOL"},
                "spendable": is_spendable_coin(coin),
                "asset": resolved_asset_filter or list_asset_id,
                "reported_asset": resolved_asset_filter,
                "scoped_asset": resolved_asset_filter,
            }
        )
    spendable = [c for c in items if bool(c.get("spendable"))]
    print(
        format_json_output(
            {
                "execution_backend": "signer",
                "market_id": market.market_id,
                "receive_address": receive_address,
                "resolved_asset_id": resolved_asset_filter,
                "coins": items,
                "coin_count": len(items),
                "spendable_coin_count": len(spendable),
                "spendable_amount": sum(int(c["amount"]) for c in spendable),
            }
        )
    )
    return 0


def coins_list(
    *,
    program_path: Path,
    asset: str | None,
    vault_id: str | None,
    cat_id: str | None = None,
) -> int:
    _ = vault_id
    program = load_program_config(program_path)
    if coin_ops_execution_backend(program) != "signer":
        print(
            format_json_output(
                {
                    "ok": False,
                    "error": "coin_list_requires_signer_backend",
                }
            ),
            file=sys.stderr,
        )
        return 2
    return _coins_list_signer(program=program, asset=asset, cat_id=cat_id)


def coin_status(
    *,
    program_path: Path,
    asset: str | None,
    vault_id: str | None,
    cat_id: str | None = None,
) -> int:
    """Show per-coin state/spendability for an optional asset scope."""
    return coins_list(
        program_path=program_path,
        asset=asset,
        vault_id=vault_id,
        cat_id=cat_id,
    )
