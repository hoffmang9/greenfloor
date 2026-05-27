from __future__ import annotations

import json

import greenfloor.offer_builder as offer_builder
from greenfloor.cli import offer_builder_sdk


def test_encode_offer_from_spend_bundle_hex_calls_native(monkeypatch) -> None:
    from greenfloor.adapters import native_offer

    captured: dict[str, bytes] = {}

    class _Native:
        @staticmethod
        def encode_offer(raw: bytes) -> str:
            captured["raw"] = raw
            return "offer1native"

    monkeypatch.setattr(native_offer, "_import_greenfloor_native", lambda: _Native())
    assert native_offer.encode_offer_from_spend_bundle_hex("aabb") == "offer1native"
    assert captured["raw"] == bytes.fromhex("aabb")


def test_build_offer_encodes_spend_bundle_hex(monkeypatch) -> None:
    def _fake_encode(raw_hex: str) -> str:
        assert raw_hex == "aa"
        return "offer1fake"

    monkeypatch.setattr(offer_builder, "encode_offer_from_spend_bundle_hex", _fake_encode)
    assert offer_builder._build_offer({"spend_bundle_hex": "aa"}) == "offer1fake"


def test_build_offer_rejects_missing_coin_backed_inputs() -> None:
    try:
        offer_builder._build_offer({"size_base_units": 10})
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert str(exc) == "missing_receive_address"


def test_build_offer_calls_coin_backed_signing(monkeypatch) -> None:
    monkeypatch.setattr(offer_builder, "_build_coin_backed_spend_bundle_hex", lambda _: "aa")
    monkeypatch.setattr(offer_builder, "encode_offer_from_spend_bundle_hex", lambda _: "offer1fake")
    offer = offer_builder._build_offer({"size_base_units": 10})
    assert offer == "offer1fake"


def test_build_offer_public_api(monkeypatch) -> None:
    monkeypatch.setattr(offer_builder, "_build_coin_backed_spend_bundle_hex", lambda _: "aa")
    monkeypatch.setattr(offer_builder, "encode_offer_from_spend_bundle_hex", lambda _: "offer1fake")
    offer = offer_builder.build_offer({"size_base_units": 10})
    assert offer == "offer1fake"


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


def test_coin_backed_signing_uses_signing_module(monkeypatch) -> None:
    """Verify _build_coin_backed_spend_bundle_hex delegates to bls_signing.build_signed_spend_bundle."""
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
