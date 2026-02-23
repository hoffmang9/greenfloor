from __future__ import annotations

import sys

import greenfloor.signing as signing_mod
from greenfloor.cli.manager import _verify_offer_text_for_dexie


def test_verify_offer_text_for_dexie_uses_greenfloor_native_when_sdk_lacks_validate(
    monkeypatch,
) -> None:
    calls: dict[str, str] = {}

    class _Native:
        @staticmethod
        def validate_offer(offer: str) -> None:
            calls["offer"] = offer

    class _Sdk:
        @staticmethod
        def verify_offer(_offer: str) -> bool:
            raise AssertionError("verify_offer fallback should not run when native is available")

    monkeypatch.setitem(sys.modules, "greenfloor_native", _Native)
    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)

    assert _verify_offer_text_for_dexie("offer1contract") is None
    assert calls["offer"] == "offer1contract"


def test_verify_offer_text_for_dexie_reports_missing_validators(monkeypatch) -> None:
    def _import_module(name: str):
        if name == "greenfloor_native":
            raise ImportError("disable native path for this test")
        return __import__(name)

    monkeypatch.setattr("greenfloor.cli.manager.importlib.import_module", _import_module)

    class _Sdk:
        pass

    monkeypatch.delitem(sys.modules, "greenfloor_native", raising=False)
    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)

    assert _verify_offer_text_for_dexie("offer1contract") == "wallet_sdk_validate_offer_unavailable"


def test_from_input_spend_bundle_xch_contract_bytes_in_bytes_out(monkeypatch) -> None:
    calls: dict[str, object] = {}

    class _InputSpendBundle:
        @staticmethod
        def to_bytes() -> bytes:
            return b"input-bytes"

    class _Native:
        @staticmethod
        def from_input_spend_bundle_xch(spend_bundle_bytes, requested):
            calls["native"] = (spend_bundle_bytes, requested)
            return b"result-bytes"

    class _SpendBundleType:
        @staticmethod
        def from_bytes(value: bytes):
            calls["from_bytes"] = value
            return "rebuilt-spend-bundle"

    class _Sdk:
        SpendBundle = _SpendBundleType

    class _Payment:
        puzzle_hash = b"\x11" * 32
        amount = 7

    class _NotarizedPayment:
        nonce = b"\x22" * 32
        payments = [_Payment()]

    monkeypatch.setattr(signing_mod, "_import_greenfloor_native", lambda: _Native)

    result = signing_mod._from_input_spend_bundle_xch(
        sdk=_Sdk,
        input_spend_bundle=_InputSpendBundle(),
        requested_payments_xch=[_NotarizedPayment()],
    )
    assert result == "rebuilt-spend-bundle"
    assert calls["native"] == (b"input-bytes", [(b"\x22" * 32, [(b"\x11" * 32, 7)])])
    assert calls["from_bytes"] == b"result-bytes"
