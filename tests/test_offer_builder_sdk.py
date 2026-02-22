from __future__ import annotations

import json

from greenfloor.cli import offer_builder_sdk


class _FakeAddressObj:
    def __init__(self, puzzle_hash: bytes) -> None:
        self.puzzle_hash = puzzle_hash


class _FakeAddress:
    @staticmethod
    def decode(value: str) -> _FakeAddressObj:
        if not value.startswith("xch1"):
            raise ValueError("bad_address")
        return _FakeAddressObj(b"\x11" * 32)


class _FakeCoin:
    def __init__(self, parent_coin_info: bytes, puzzle_hash: bytes, amount: int) -> None:
        self.parent_coin_info = parent_coin_info
        self.puzzle_hash = puzzle_hash
        self.amount = amount


class _FakeCoinSpend:
    def __init__(self, coin: _FakeCoin, puzzle_reveal: bytes, solution: bytes) -> None:
        self.coin = coin
        self.puzzle_reveal = puzzle_reveal
        self.solution = solution


class _FakeSignature:
    @staticmethod
    def infinity() -> str:
        return "sig"


class _FakeSpendBundle:
    def __init__(self, coin_spends, aggregated_signature) -> None:
        self.coin_spends = coin_spends
        self.aggregated_signature = aggregated_signature

    @staticmethod
    def from_bytes(value: bytes) -> _FakeSpendBundle:
        if value != b"\xaa":
            raise ValueError("bad_spend_bundle_bytes")
        coin = _FakeCoin(b"\x01" * 32, b"\x02" * 32, 10)
        return _FakeSpendBundle([_FakeCoinSpend(coin, b"\x80", b"\x80")], "sig")


class _FakeSdk:
    Address = _FakeAddress
    Coin = _FakeCoin
    CoinSpend = _FakeCoinSpend
    Signature = _FakeSignature
    SpendBundle = _FakeSpendBundle

    @staticmethod
    def from_hex(value: str) -> bytes:
        if value != "aa":
            raise ValueError("bad_hex")
        return b"\xaa"

    @staticmethod
    def encode_offer(spend_bundle: _FakeSpendBundle) -> str:
        assert len(spend_bundle.coin_spends) == 1
        assert spend_bundle.coin_spends[0].coin.amount == 10
        return "offer1fake"


def test_build_offer_success_with_wallet_sdk_types() -> None:
    offer = offer_builder_sdk._build_offer({"spend_bundle_hex": "aa"}, _FakeSdk)
    assert offer == "offer1fake"


def test_build_offer_rejects_missing_coin_backed_inputs() -> None:
    try:
        offer_builder_sdk._build_offer({"size_base_units": 10}, _FakeSdk)
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert str(exc) == "missing_receive_address"


def test_build_offer_calls_coin_backed_signing(monkeypatch) -> None:
    monkeypatch.setattr(offer_builder_sdk, "_build_coin_backed_spend_bundle_hex", lambda _: "aa")
    offer = offer_builder_sdk._build_offer({"size_base_units": 10}, _FakeSdk)
    assert offer == "offer1fake"


def test_build_offer_public_api(monkeypatch) -> None:
    monkeypatch.setattr(offer_builder_sdk, "_import_sdk", lambda: _FakeSdk)
    monkeypatch.setattr(offer_builder_sdk, "_build_coin_backed_spend_bundle_hex", lambda _: "aa")
    offer = offer_builder_sdk.build_offer({"size_base_units": 10})
    assert offer == "offer1fake"


def test_main_outputs_executed_json(monkeypatch, capsys) -> None:
    monkeypatch.setattr(offer_builder_sdk, "_import_sdk", lambda: _FakeSdk)
    monkeypatch.setattr(offer_builder_sdk, "_build_coin_backed_spend_bundle_hex", lambda _: "aa")
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
    """Verify _build_coin_backed_spend_bundle_hex delegates to signing.build_signed_spend_bundle."""
    import greenfloor.signing as signing_mod

    captured = {}

    def _fake_build(payload):
        captured["payload"] = payload
        return {
            "status": "executed",
            "reason": "ok",
            "spend_bundle_hex": "deadbeef",
        }

    monkeypatch.setattr(signing_mod, "build_signed_spend_bundle", _fake_build)
    result = offer_builder_sdk._build_coin_backed_spend_bundle_hex(
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
