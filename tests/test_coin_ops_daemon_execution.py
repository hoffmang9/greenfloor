from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Any

from greenfloor.config.models import MarketConfig, MarketInventoryConfig
from greenfloor.core.coin_ops import CoinOpPlan
from greenfloor.runtime.coin_ops.daemon_execution import (
    DaemonCoinOpExecContext,
    execute_daemon_split_plan,
)
from greenfloor.runtime.coin_ops.models import CoinOpSelectionMode
from greenfloor.runtime.coin_ops_backend import CoinOpScope


def _market() -> MarketConfig:
    return MarketConfig(
        market_id="m1",
        enabled=True,
        base_asset="asset",
        base_symbol="BYC",
        quote_asset="xch",
        quote_asset_type="unstable",
        receive_address="xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        mode="sell_only",
        signer_key_id="key-main-1",
        inventory=MarketInventoryConfig(low_watermark_base_units=100),
        pricing={
            "fixed_quote_per_base": 0.5,
            "base_unit_mojo_multiplier": 1000,
            "quote_unit_mojo_multiplier": 1000,
        },
    )


class _Program:
    coin_ops_split_fee_mojos = 0
    coin_ops_combine_fee_mojos = 0


@dataclass
class _FakeCloudWallet:
    combine_calls: int = 0
    split_calls: int = 0

    def list_coins(self, *, asset_id: str | None = None, include_pending: bool = True):
        _ = asset_id, include_pending
        return [
            {"id": "Coin_a", "amount": 25_000, "state": "SETTLED"},
            {"id": "Coin_b", "amount": 8_000, "state": "SETTLED"},
            {"id": "Coin_c", "amount": 12_000, "state": "SETTLED"},
        ]

    def split_coins(
        self,
        *,
        coin_ids: list[str],
        amount_per_coin: int,
        number_of_coins: int,
        fee: int,
    ) -> dict[str, Any]:
        _ = amount_per_coin, number_of_coins, fee
        self.split_calls += 1
        assert coin_ids == ["Coin_a"]
        raise RuntimeError("Some selected coins are not spendable")

    def combine_coins(self, **_kwargs: Any) -> dict[str, Any]:
        self.combine_calls += 1
        raise AssertionError("combine should not run on split retry")


@dataclass
class _FakeCoinOpBackend:
    scope: CoinOpScope
    wallet: _FakeCloudWallet
    resolved_asset_id: str = "Asset_byc"

    def list_asset_scoped_coins(self) -> list[dict[str, Any]]:
        return self.wallet.list_coins(asset_id=self.resolved_asset_id)

    def filter_spendable(
        self,
        coins: list[dict[str, Any]],
        *,
        canonical_asset_id: str,
        min_coin_amount_mojos: int,
        mode: CoinOpSelectionMode,
        verify_direct_spendable_lookup: bool = False,
    ) -> list[dict[str, Any]]:
        _ = canonical_asset_id, min_coin_amount_mojos, mode, verify_direct_spendable_lookup
        return list(coins)

    def split_coins(
        self,
        *,
        coin_ids: list[str],
        amount_per_coin: int,
        number_of_coins: int,
        fee_mojos: int,
        initial_coin_ids: set[str] | None = None,
    ) -> dict[str, Any]:
        _ = initial_coin_ids
        return self.wallet.split_coins(
            coin_ids=coin_ids,
            amount_per_coin=amount_per_coin,
            number_of_coins=number_of_coins,
            fee=fee_mojos,
        )

    def combine_coins(self, **_kwargs: Any) -> dict[str, Any]:
        return self.wallet.combine_coins(**_kwargs)


def test_execute_daemon_split_plan_retry_disables_combine_prereq() -> None:
    """After a spendable retry, attempt 2 must not submit combine-for-split."""

    wallet = _FakeCloudWallet()
    market = _market()
    backend = _FakeCoinOpBackend(
        scope=CoinOpScope(
            market=market,
            selected_venue=None,
        ),
        wallet=wallet,
    )
    ctx = DaemonCoinOpExecContext(
        backend=backend,  # type: ignore[arg-type]
        market=market,
        program=_Program(),  # type: ignore[arg-type]
        resolved_base_asset_id="Asset_byc",
        base_unit_mojo_multiplier=1000,
        combine_input_cap=10,
        watched_coin_ids=set(),
        logger=logging.getLogger("test.coin_ops_daemon_execution"),
    )
    plan = CoinOpPlan(op_type="split", size_base_units=10, op_count=2, reason="r")

    items, executed_count = execute_daemon_split_plan(plan=plan, ctx=ctx)

    assert wallet.split_calls == 1
    assert wallet.combine_calls == 0
    assert executed_count == 0
    assert len(items) == 1
    assert items[0]["status"] == "skipped"
    assert items[0]["reason"] == "no_spendable_split_coin_meets_required_amount"
