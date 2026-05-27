from __future__ import annotations

import json

import greenfloor.offer_builder as offer_builder
from greenfloor.cli import offer_builder_sdk


def test_build_offer_rejects_missing_coin_backed_inputs() -> None:
    try:
        offer_builder._build_offer({"size_base_units": 10})
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert str(exc) == "missing_receive_address"


def test_build_coin_backed_spend_bundle_hex_maps_payload(monkeypatch) -> None:
    import greenfloor.adapters.bls_signing as bls_signing_mod

    captured = {}

    def _fake_build(payload):
        captured["payload"] = payload
        return {
            "status": "executed",
            "reason": "ok",
            "spend_bundle_hex": "deadbeef",
        }

    monkeypatch.setattr(bls_signing_mod, "build_signed_spend_bundle", _fake_build)
    result = offer_builder._build_coin_backed_spend_bundle_hex(
        {
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
    )
    assert result == "deadbeef"
    assert captured["payload"]["key_id"] == "k1"
    assert captured["payload"]["plan"]["op_type"] == "offer"
    assert captured["payload"]["plan"]["offer_amount"] == 10000


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
