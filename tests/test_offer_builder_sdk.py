from __future__ import annotations

import json

import pytest

import greenfloor.offer_builder as offer_builder
from greenfloor.cli import offer_builder_sdk

_BLS_COIN_BACKED_BASE = {
    "receive_address": "xch1abc",
    "key_id": "k1",
    "network": "mainnet",
    "keyring_yaml_path": "/tmp/k.yaml",
    "size_base_units": 10,
    "asset_id": "xch",
    "quote_asset": "xch",
    "quote_price_quote_per_base": 0.5,
    "base_unit_mojo_multiplier": 1000,
    "quote_unit_mojo_multiplier": 1000,
}


def test_build_offer_rejects_missing_coin_backed_inputs() -> None:
    try:
        offer_builder._build_offer({"size_base_units": 10})
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert str(exc) == "missing_receive_address"


@pytest.mark.parametrize(
    ("field", "value", "message"),
    [
        ("size_base_units", 0, "invalid_size_base_units"),
        ("quote_price_quote_per_base", 0.0, "invalid_quote_price_quote_per_base"),
        ("base_unit_mojo_multiplier", 0, "invalid_base_unit_mojo_multiplier"),
        ("quote_unit_mojo_multiplier", 0, "invalid_quote_unit_mojo_multiplier"),
    ],
)
def test_legacy_action_request_rejects_invalid_plan_inputs(
    field: str,
    value: int | float,
    message: str,
) -> None:
    from greenfloor.core import offer_action as offer_action_core

    payload = dict(_BLS_COIN_BACKED_BASE)
    payload[field] = value
    with pytest.raises(ValueError, match=message):
        offer_action_core.validate_legacy_offer_payload(payload)


def test_build_offer_calls_kernel_action(monkeypatch) -> None:
    captured: dict = {}

    def _fake_build(*, network: str, key_id: str, request: dict, config_path=None) -> dict:
        captured["network"] = network
        captured["key_id"] = key_id
        captured["request"] = request
        captured["config_path"] = config_path
        return {"offer_text": "offer1fake"}

    monkeypatch.setattr(
        "greenfloor.offer_builder.build_bls_offer_for_action",
        _fake_build,
    )
    offer_text = offer_builder.build_offer(dict(_BLS_COIN_BACKED_BASE))
    assert offer_text == "offer1fake"
    assert captured["network"] == "mainnet"
    assert captured["key_id"] == "k1"
    assert captured["request"]["size_base_units"] == 10


def test_main_outputs_executed_json(monkeypatch, capsys) -> None:
    monkeypatch.setattr(offer_builder_sdk, "build_offer", lambda _payload: "offer1fake")
    monkeypatch.setattr(
        offer_builder_sdk.sys,
        "stdin",
        type(
            "_In",
            (),
            {"read": lambda self: json.dumps({"receive_address": "xch1ok", "size_base_units": 10})},
        )(),
    )

    offer_builder_sdk.main()
    out = json.loads(capsys.readouterr().out.strip())
    assert out["status"] == "executed"
    assert out["offer"] == "offer1fake"
