from __future__ import annotations

import logging
from dataclasses import dataclass
from types import SimpleNamespace
from typing import Any

from greenfloor.config.models import MarketConfig, MarketInventoryConfig
from greenfloor.core.coin_ops import CoinOpPlan
from greenfloor.runtime.coin_ops.daemon_execution import (
    DaemonCoinOpExecContext,
    execute_daemon_combine_plan,
    execute_daemon_split_plan,
    execute_managed_coin_op_plans,
)
from greenfloor.runtime.coin_ops_backend import (
    CoinOpScope,
    SignerCoinOpBackend,
    build_coin_op_backend,
    resolve_coin_op_base_asset_id,
)


def _signer_market(*, receive_address: str = "xch1test") -> MarketConfig:
    return MarketConfig(
        market_id="m-signer",
        enabled=True,
        base_asset="asset",
        base_symbol="BYC",
        quote_asset="xch",
        quote_asset_type="unstable",
        receive_address=receive_address,
        mode="sell_only",
        signer_key_id="key-main-1",
        inventory=MarketInventoryConfig(low_watermark_base_units=100),
        pricing={
            "fixed_quote_per_base": 0.5,
            "base_unit_mojo_multiplier": 1000,
            "quote_unit_mojo_multiplier": 1000,
        },
    )


@dataclass
class _SignerProgram:
    runtime_dry_run = False
    app_network = "mainnet"
    signer_kms_key_id = "kms-1"
    vault_config = SimpleNamespace(launcher_id="0" * 64)
    home_dir = "/tmp/greenfloor-test"
    coin_ops_split_fee_mojos = 0
    coin_ops_combine_fee_mojos = 0


class _SignerSelection:
    key_id = "key-main-1"


def test_coin_op_scope_disallows_signer_split_combine_prereq() -> None:
    scope = CoinOpScope(
        market=_signer_market(),
        selected_venue=None,
        vault_id="signer",
    )
    assert scope.allows_daemon_split_combine_prereq is True
    assert scope.split_submitted_reason() == "signer_split_submitted"
    assert scope.combine_prereq_submitted_reason(exact_match=True) == (
        "signer_combine_submitted_for_split_prereq_exact"
    )


def test_execute_managed_coin_op_plans_missing_receive_address() -> None:
    market = _signer_market(receive_address="")
    result = execute_managed_coin_op_plans(
        market=market,
        program=_SignerProgram(),  # type: ignore[arg-type]
        plans=[CoinOpPlan(op_type="split", size_base_units=10, op_count=2, reason="r")],
        signer_selection=_SignerSelection(),
        base_unit_mojo_multiplier=1000,
        combine_input_cap=10,
        watched_coin_ids=set(),
        logger=logging.getLogger("test.signer.coin_ops"),
    )
    assert result["executed_count"] == 0
    assert result["items"][0]["reason"] == "signer_coin_ops_missing_receive_address"


def test_signer_daemon_split_submits(monkeypatch) -> None:
    market = _signer_market()
    market.base_asset = "d" * 64
    program = _SignerProgram()

    monkeypatch.setattr(
        "greenfloor.runtime.signer_coin_op_backend.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )
    monkeypatch.setattr(
        "greenfloor.runtime.signer_coin_op_backend.list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"id": "coin_a", "name": "coin_a", "amount": 50_000, "state": "CONFIRMED"},
        ],
    )
    monkeypatch.setattr(
        "greenfloor.runtime.signer_coin_op_backend.rust_signer.build_mixed_split",
        lambda *_args, **_kwargs: {
            "spend_bundle_hex": "0x" + ("ab" * 64),
            "broadcast_status": "submitted",
        },
    )
    monkeypatch.setattr(
        "greenfloor.runtime.signer_coin_op_backend._operation_id_from_spend_bundle_hex",
        lambda _hex: "op-split-1",
    )

    backend = build_coin_op_backend(
        program=program,  # type: ignore[arg-type]
        market=market,
        selected_venue=None,
        resolved_asset_id="asset_byc",
    )
    assert isinstance(backend, SignerCoinOpBackend)
    ctx = DaemonCoinOpExecContext(
        backend=backend,
        market=market,
        program=program,  # type: ignore[arg-type]
        resolved_base_asset_id="asset_byc",
        base_unit_mojo_multiplier=1000,
        combine_input_cap=10,
        watched_coin_ids=set(),
        logger=logging.getLogger("test.signer.coin_ops"),
    )
    plan = CoinOpPlan(op_type="split", size_base_units=10, op_count=4, reason="r")
    items, executed = execute_daemon_split_plan(plan=plan, ctx=ctx)

    assert executed == 1
    assert items[0]["status"] == "executed"
    assert items[0]["reason"] == "signer_split_submitted"
    assert items[0]["operation_id"] == "op-split-1"


def test_signer_daemon_split_skips_combine_prereq_when_only_small_coins(monkeypatch) -> None:
    """Signer path must not submit combine-for-split (Cloud Wallet-only prereq)."""
    market = _signer_market()
    program = _SignerProgram()
    combine_calls: list[dict[str, Any]] = []

    def _fake_combine(**kwargs: Any) -> dict[str, Any]:
        combine_calls.append(dict(kwargs))
        return {"operation_id": "should-not-run"}

    monkeypatch.setattr(
        "greenfloor.runtime.signer_coin_op_backend.list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"id": "small_a", "name": "small_a", "amount": 8_000, "state": "CONFIRMED"},
            {"id": "small_b", "name": "small_b", "amount": 12_000, "state": "CONFIRMED"},
        ],
    )
    monkeypatch.setattr(
        SignerCoinOpBackend,
        "combine_coins",
        lambda self, **kwargs: _fake_combine(**kwargs),
    )

    backend = SignerCoinOpBackend(
        program=program,  # type: ignore[arg-type]
        market=market,
        selected_venue=None,
        resolved_asset_id="asset_byc",
        receive_address=market.receive_address,
    )
    ctx = DaemonCoinOpExecContext(
        backend=backend,
        market=market,
        program=program,  # type: ignore[arg-type]
        resolved_base_asset_id="asset_byc",
        base_unit_mojo_multiplier=1000,
        combine_input_cap=10,
        watched_coin_ids=set(),
        logger=logging.getLogger("test.signer.coin_ops"),
    )
    plan = CoinOpPlan(op_type="split", size_base_units=10, op_count=4, reason="r")
    items, executed = execute_daemon_split_plan(plan=plan, ctx=ctx)

    assert combine_calls == []
    assert executed == 0
    assert items[0]["status"] == "skipped"
    assert items[0]["reason"] == "no_spendable_split_coin_meets_required_amount"


def test_signer_daemon_combine_submits(monkeypatch) -> None:
    market = _signer_market()
    program = _SignerProgram()
    captured: dict[str, Any] = {}

    monkeypatch.setattr(
        "greenfloor.runtime.signer_coin_op_backend.list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"id": "c1", "name": "c1", "amount": 1_000, "state": "CONFIRMED"},
            {"id": "c2", "name": "c2", "amount": 1_000, "state": "CONFIRMED"},
        ],
    )

    def _fake_mixed_split(self: SignerCoinOpBackend, **kwargs: Any) -> dict[str, Any]:
        captured.update(kwargs)
        return {"operation_id": "op-combine-1", "signature_request_id": "op-combine-1"}

    monkeypatch.setattr(SignerCoinOpBackend, "_execute_mixed_split", _fake_mixed_split)

    backend = SignerCoinOpBackend(
        program=program,  # type: ignore[arg-type]
        market=market,
        selected_venue=None,
        resolved_asset_id="asset_byc",
        receive_address=market.receive_address,
    )
    ctx = DaemonCoinOpExecContext(
        backend=backend,
        market=market,
        program=program,  # type: ignore[arg-type]
        resolved_base_asset_id="asset_byc",
        base_unit_mojo_multiplier=1000,
        combine_input_cap=10,
        watched_coin_ids=set(),
        logger=logging.getLogger("test.signer.coin_ops"),
    )
    plan = CoinOpPlan(op_type="combine", size_base_units=1, op_count=2, reason="r")
    items, executed = execute_daemon_combine_plan(plan=plan, ctx=ctx)

    assert executed == 1
    assert items[0]["reason"] == "signer_combine_submitted"
    assert captured["output_amounts"] == [1000, 1000]


def test_resolve_coin_op_base_asset_id_signer_xch() -> None:
    market = _signer_market()
    market.base_asset = "xch"
    assert (
        resolve_coin_op_base_asset_id(
            program=_SignerProgram(),  # type: ignore[arg-type]
            market=market,
        )
        == "xch"
    )
