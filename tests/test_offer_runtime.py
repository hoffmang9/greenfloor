"""Lean orchestration tests for the local Rust signer offer runtime."""

from __future__ import annotations

from dataclasses import replace
from types import SimpleNamespace
from typing import cast

import pytest

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.runtime.offer_runtime import signer_create_offer_phase
from tests.helpers.config_fixtures import minimal_market_config


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
        return {"offer": "offer1test", "execution_mode": "direct"}

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
        market=market,
        size_base_units=10,
        quote_price=2.0,
        resolved_base_asset_id="basecat",
        resolved_quote_asset_id="quotecat",
        expiry_unit="hours",
        expiry_value=1,
        action_side="buy",
    )

    assert captured
    assert captured["receive_address"] == market.receive_address
    assert captured["expires_at"] is not None
    assert result["side"] == "buy"
    assert result["offer_text"] == "offer1test"
    assert result["execution_mode"] == "direct"
    assert result["expires_at"]


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
            market=_sample_market(),
            size_base_units=1,
            quote_price=1.0,
            resolved_base_asset_id="basecat",
            resolved_quote_asset_id="xch",
            expiry_unit="hours",
            expiry_value=1,
        )
