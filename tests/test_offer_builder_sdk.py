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


class _FakeSdk:
    Address = _FakeAddress
    Coin = _FakeCoin
    CoinSpend = _FakeCoinSpend
    Signature = _FakeSignature
    SpendBundle = _FakeSpendBundle

    @staticmethod
    def encode_offer(spend_bundle: _FakeSpendBundle) -> str:
        assert len(spend_bundle.coin_spends) == 1
        assert spend_bundle.coin_spends[0].coin.amount == 10
        return "offer1fake"


def test_build_offer_success_with_wallet_sdk_types() -> None:
    offer = offer_builder_sdk._build_offer(
        {
            "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            "size_base_units": 10,
            "pair": "xch",
        },
        _FakeSdk,
    )
    assert offer == "offer1fake"


def test_build_offer_rejects_missing_address() -> None:
    try:
        offer_builder_sdk._build_offer({"size_base_units": 10}, _FakeSdk)
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert str(exc) == "missing_receive_address"


def test_main_outputs_executed_json(monkeypatch, capsys) -> None:
    monkeypatch.setattr(offer_builder_sdk, "_import_sdk", lambda: _FakeSdk)
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
