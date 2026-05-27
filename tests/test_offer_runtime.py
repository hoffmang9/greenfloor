"""Lean orchestration tests for the local Rust signer offer runtime."""

from __future__ import annotations

from types import SimpleNamespace
from typing import cast
from unittest.mock import MagicMock

import pytest

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.runtime.offer_runtime import signer_create_offer_phase


def _sample_market(*, base_multiplier: int = 1000, quote_multiplier: int = 1000) -> SimpleNamespace:
    return SimpleNamespace(
        receive_address="txch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqstg4h8",
        pricing={
            "base_unit_mojo_multiplier": base_multiplier,
            "quote_unit_mojo_multiplier": quote_multiplier,
        },
    )


def test_signer_create_offer_phase_buy_side_swaps_legs(monkeypatch) -> None:
    captured: dict = {}

    def _fake_build(_config_path: str, request: dict) -> dict:
        captured.update(request)
        return {"offer": "offer1test", "execution_mode": "direct"}

    fake_signer = MagicMock()
    fake_signer.build_vault_cat_offer.side_effect = _fake_build
    monkeypatch.setattr(
        "greenfloor.adapters.rust_signer.build_vault_cat_offer",
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
        market=cast(MarketConfig, market),
        size_base_units=10,
        quote_price=2.0,
        resolved_base_asset_id="basecat",
        resolved_quote_asset_id="quotecat",
        expiry_unit="hours",
        expiry_value=1,
        action_side="buy",
    )

    assert result["side"] == "buy"
    assert captured["offer_asset_id"] == "quotecat"
    assert captured["request_asset_id"] == "basecat"
    assert captured["offer_amount"] == 20_000
    assert captured["request_amount"] == 10_000
    assert captured["split_input_coins"] is True
    assert result["offer_text"] == "offer1test"


def test_signer_create_offer_phase_sell_side_keeps_legs(monkeypatch) -> None:
    captured: dict = {}

    def _fake_build(_config_path: str, request: dict) -> dict:
        captured.update(request)
        return {"offer": "offer1sell", "execution_mode": "direct"}

    fake_signer = MagicMock()
    fake_signer.build_vault_cat_offer.side_effect = _fake_build
    monkeypatch.setattr(
        "greenfloor.adapters.rust_signer.build_vault_cat_offer",
        _fake_build,
    )
    monkeypatch.setattr(
        "greenfloor.runtime.offer_runtime.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )

    program = cast(ProgramConfig, SimpleNamespace())
    market = _sample_market()
    signer_create_offer_phase(
        program=program,
        market=cast(MarketConfig, market),
        size_base_units=5,
        quote_price=1.5,
        resolved_base_asset_id="basecat",
        resolved_quote_asset_id="quotecat",
        expiry_unit="minutes",
        expiry_value=30,
        action_side="sell",
    )

    assert captured["offer_asset_id"] == "basecat"
    assert captured["request_asset_id"] == "quotecat"
    assert captured["offer_amount"] == 5_000
    assert captured["request_amount"] == 7_500


def test_signer_create_offer_phase_requires_offer_text(monkeypatch) -> None:
    monkeypatch.setattr(
        "greenfloor.adapters.rust_signer.build_vault_cat_offer",
        lambda _path, _req: {"offer": "", "execution_mode": "direct"},
    )
    monkeypatch.setattr(
        "greenfloor.runtime.offer_runtime.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )

    with pytest.raises(RuntimeError, match="missing_offer_text"):
        signer_create_offer_phase(
            program=cast(ProgramConfig, SimpleNamespace()),
            market=cast(MarketConfig, _sample_market()),
            size_base_units=1,
            quote_price=1.0,
            resolved_base_asset_id="basecat",
            resolved_quote_asset_id="xch",
            expiry_unit="hours",
            expiry_value=1,
        )
