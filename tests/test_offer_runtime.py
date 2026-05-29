"""Lean orchestration tests for the local Rust signer offer runtime."""

from __future__ import annotations

from dataclasses import replace
from types import SimpleNamespace
from typing import Any, cast

import pytest

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.runtime.offer_bootstrap import BootstrapRuntimeDeps
from greenfloor.runtime.offer_runtime import (
    default_bootstrap_runtime_deps,
    signer_bootstrap_phase,
    signer_create_offer_phase,
)
from tests.helpers.config_fixtures import (
    minimal_market_config,
    minimal_market_with_sell_ladder,
    minimal_market_with_tiered_sell_ladder,
    minimal_program_config,
)


def _sample_market(*, base_multiplier: int = 1000, quote_multiplier: int = 1000) -> MarketConfig:
    return replace(
        minimal_market_config(),
        receive_address="txch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqstg4h8",
        pricing={
            "base_unit_mojo_multiplier": base_multiplier,
            "quote_unit_mojo_multiplier": quote_multiplier,
        },
    )


def test_signer_create_offer_phase_calls_signer_and_returns_offer_text(monkeypatch) -> None:
    captured: dict = {}

    def _fake_build(_config_path: str, request: dict) -> dict:
        captured.update(request)
        return {
            "offer_text": "offer1test",
            "execution_mode": "direct",
            "side": "buy",
            "expires_at_unix": 1_700_000_000,
            "offer_amount": 10_000,
            "request_amount": 20_000,
            "create_result": {"execution_mode": "direct"},
        }

    monkeypatch.setattr(
        "greenfloor.adapters.offer_action.build_signer_offer_for_action",
        _fake_build,
    )
    monkeypatch.setattr(
        "greenfloor.runtime.offer_runtime.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )

    program = cast(ProgramConfig, SimpleNamespace())
    market = _sample_market()
    result = signer_create_offer_phase(
        program=program,
        market=market,
        size_base_units=10,
        quote_price=2.0,
        resolved_base_asset_id="basecat",
        resolved_quote_asset_id="quotecat",
        action_side="buy",
    )

    assert captured
    assert captured["receive_address"] == market.receive_address
    assert captured["base_asset"] == "basecat"
    assert captured["quote_asset"] == "quotecat"
    assert result["side"] == "buy"
    assert result["offer_text"] == "offer1test"
    assert result["execution_mode"] == "direct"
    assert result["expires_at"]


def test_signer_create_offer_phase_requires_offer_text(monkeypatch) -> None:
    def _raise_missing(_path: str, _req: dict) -> dict:
        raise RuntimeError("offer_action_failed:missing_offer_text")

    monkeypatch.setattr(
        "greenfloor.adapters.offer_action.build_signer_offer_for_action",
        _raise_missing,
    )
    monkeypatch.setattr(
        "greenfloor.runtime.offer_runtime.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )

    with pytest.raises(RuntimeError, match="offer_action_failed:missing_offer_text"):
        signer_create_offer_phase(
            program=cast(ProgramConfig, SimpleNamespace()),
            market=_sample_market(),
            size_base_units=1,
            quote_price=1.0,
            resolved_base_asset_id="basecat",
            resolved_quote_asset_id="xch",
        )


def _spendable_coin(*, coin_id: str, amount: int) -> dict[str, object]:
    return {"id": coin_id, "amount": amount, "state": "CONFIRMED"}


def _bootstrap_deps(**overrides: object) -> BootstrapRuntimeDeps:
    return replace(default_bootstrap_runtime_deps(), **overrides)


def test_signer_bootstrap_phase_skips_when_planner_reports_ready() -> None:
    market = minimal_market_with_sell_ladder(size_base_units=10, target_count=1)
    program = minimal_program_config()
    spendable = [_spendable_coin(coin_id="coin-10", amount=10)]

    result = signer_bootstrap_phase(
        program=program,
        market=market,
        resolved_base_asset_id="basecat",
        resolved_quote_asset_id="xch",
        quote_price=1.0,
        bootstrap_deps=_bootstrap_deps(list_bootstrap_coins_fn=lambda **_kwargs: spendable),
        resolve_bootstrap_split_fee_fn=lambda **_kwargs: (0, "zero", None),
    )

    assert result.status == "skipped"
    assert result.reason == "already_ready"


def test_signer_bootstrap_phase_skips_when_underfunded() -> None:
    market = minimal_market_with_sell_ladder(size_base_units=10, target_count=2)
    program = minimal_program_config()
    spendable = [_spendable_coin(coin_id="coin-small", amount=5)]

    result = signer_bootstrap_phase(
        program=program,
        market=market,
        resolved_base_asset_id="basecat",
        resolved_quote_asset_id="xch",
        quote_price=1.0,
        bootstrap_deps=_bootstrap_deps(list_bootstrap_coins_fn=lambda **_kwargs: spendable),
        resolve_bootstrap_split_fee_fn=lambda **_kwargs: (0, "zero", None),
    )

    assert result.status == "skipped"
    assert result.reason == "bootstrap_underfunded:total_output_amount=20"


def test_signer_bootstrap_phase_blocks_nonzero_fee_before_split() -> None:
    market = minimal_market_with_sell_ladder(size_base_units=10, target_count=2)
    program = minimal_program_config()
    spendable = [_spendable_coin(coin_id="coin-big", amount=1000)]

    result = signer_bootstrap_phase(
        program=program,
        market=market,
        resolved_base_asset_id="basecat",
        resolved_quote_asset_id="xch",
        quote_price=1.0,
        bootstrap_deps=_bootstrap_deps(list_bootstrap_coins_fn=lambda **_kwargs: spendable),
        resolve_bootstrap_split_fee_fn=lambda **_kwargs: (1, "coinset", None),
    )

    assert result.status == "failed"
    assert result.reason == "signer_mixed_split_fee_not_supported"


def test_signer_bootstrap_phase_submits_planner_mixed_output_amounts(monkeypatch) -> None:
    market = minimal_market_with_tiered_sell_ladder()
    program = minimal_program_config()
    spendable = [
        _spendable_coin(coin_id="coin-small-1", amount=1),
        _spendable_coin(coin_id="coin-big", amount=1000),
        _spendable_coin(coin_id="coin-hundred", amount=100),
    ]
    captured: dict[str, Any] = {}

    def _fake_split(_config_path: str, request: dict[str, Any]) -> dict[str, str]:
        captured.update(request)
        return {"status": "executed"}

    monkeypatch.setattr(
        "greenfloor.adapters.rust_signer.build_mixed_split",
        _fake_split,
    )
    monkeypatch.setattr(
        "greenfloor.runtime.offer_runtime.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )

    result = signer_bootstrap_phase(
        program=program,
        market=market,
        resolved_base_asset_id="basecat",
        resolved_quote_asset_id="xch",
        quote_price=1.0,
        bootstrap_deps=_bootstrap_deps(
            list_bootstrap_coins_fn=lambda **_kwargs: spendable,
            wait_for_confirmation_fn=lambda **_kwargs: [],
        ),
        resolve_bootstrap_split_fee_fn=lambda **_kwargs: (0, "zero", None),
    )

    assert result.status == "executed"
    assert captured["output_amounts"] == [1, 1, 10, 10, 10]
    assert captured["coin_ids"] == ["coin-big"]


def test_signer_bootstrap_phase_submits_single_planner_output(monkeypatch) -> None:
    market = minimal_market_with_sell_ladder(size_base_units=10, target_count=1)
    program = minimal_program_config()
    spendable = [_spendable_coin(coin_id="coin-big", amount=100)]
    captured: dict[str, Any] = {}

    def _fake_split(_config_path: str, request: dict[str, Any]) -> dict[str, str]:
        captured.update(request)
        return {"status": "executed"}

    monkeypatch.setattr(
        "greenfloor.adapters.rust_signer.build_mixed_split",
        _fake_split,
    )
    monkeypatch.setattr(
        "greenfloor.runtime.offer_runtime.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )

    result = signer_bootstrap_phase(
        program=program,
        market=market,
        resolved_base_asset_id="basecat",
        resolved_quote_asset_id="xch",
        quote_price=1.0,
        bootstrap_deps=_bootstrap_deps(
            list_bootstrap_coins_fn=lambda **_kwargs: spendable,
            wait_for_confirmation_fn=lambda **_kwargs: [],
        ),
        resolve_bootstrap_split_fee_fn=lambda **_kwargs: (0, "zero", None),
    )

    assert result.status == "executed"
    assert captured["output_amounts"] == [10]


def test_signer_bootstrap_phase_executes_split_from_planner_deficit(monkeypatch) -> None:
    market = minimal_market_with_sell_ladder(size_base_units=10, target_count=2)
    program = minimal_program_config()
    initial_spendable = [_spendable_coin(coin_id="coin-big", amount=1000)]
    refreshed_spendable = [
        _spendable_coin(coin_id="coin-10-a", amount=10),
        _spendable_coin(coin_id="coin-10-b", amount=10),
    ]
    captured: dict[str, Any] = {}
    list_calls = {"count": 0}

    def _list_coins(**_kwargs: object) -> list[dict[str, object]]:
        list_calls["count"] += 1
        if list_calls["count"] == 1:
            return initial_spendable
        return refreshed_spendable

    def _fake_split(_config_path: str, request: dict[str, Any]) -> dict[str, str]:
        captured.update(request)
        return {"status": "executed"}

    monkeypatch.setattr(
        "greenfloor.adapters.rust_signer.build_mixed_split",
        _fake_split,
    )
    monkeypatch.setattr(
        "greenfloor.runtime.offer_runtime.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )

    result = signer_bootstrap_phase(
        program=program,
        market=market,
        resolved_base_asset_id="basecat",
        resolved_quote_asset_id="xch",
        quote_price=1.0,
        bootstrap_deps=_bootstrap_deps(
            list_bootstrap_coins_fn=_list_coins,
            wait_for_confirmation_fn=lambda **_kwargs: [{"coin_id": "coin-10-a", "amount": "10"}],
        ),
        resolve_bootstrap_split_fee_fn=lambda **_kwargs: (0, "zero", None),
    )

    assert result.status == "executed"
    assert result.reason == "bootstrap_submitted"
    assert captured["coin_ids"] == ["coin-big"]
    assert captured["output_amounts"] == [10, 10]
    assert result.plan is not None
    assert result.plan.source_coin_id == "coin-big"
    assert result.ready is True
