from __future__ import annotations

import sys

import greenfloor.adapters.native_offer as native_offer_mod
from greenfloor.runtime.offer_publish import (
    verify_offer_text_for_dexie as _verify_offer_text_for_dexie,
)


def test_verify_offer_text_for_dexie_uses_greenfloor_signer_when_sdk_lacks_validate(
    monkeypatch,
) -> None:
    calls: dict[str, str] = {}

    class _Signer:
        @staticmethod
        def validate_offer(offer: str) -> None:
            calls["offer"] = offer

    class _Sdk:
        @staticmethod
        def verify_offer(_offer: str) -> bool:
            raise AssertionError("verify_offer fallback should not run when signer is available")

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Signer)
    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)

    assert _verify_offer_text_for_dexie("offer1contract") is None
    assert calls["offer"] == "offer1contract"


def test_verify_offer_text_for_dexie_reports_missing_validators(monkeypatch) -> None:
    def _import_module(name: str):
        if name == "greenfloor_signer":
            raise ImportError("disable signer path for this test")
        return __import__(name)

    monkeypatch.setattr(
        "greenfloor.runtime.offer_publish.importlib.import_module",
        _import_module,
    )

    class _Sdk:
        pass

    monkeypatch.delitem(sys.modules, "greenfloor_signer", raising=False)
    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)

    assert _verify_offer_text_for_dexie("offer1contract") == "wallet_sdk_validate_offer_unavailable"


def test_from_input_spend_bundle_xch_contract_bytes_in_bytes_out(monkeypatch) -> None:
    calls: dict[str, object] = {}

    class _InputSpendBundle:
        @staticmethod
        def to_bytes() -> bytes:
            return b"input-bytes"

    class _Signer:
        @staticmethod
        def from_input_spend_bundle_xch(spend_bundle_bytes, requested):
            calls["signer"] = (spend_bundle_bytes, requested)
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

    monkeypatch.setattr(native_offer_mod, "_import_greenfloor_signer", lambda: _Signer)

    result = native_offer_mod.from_input_spend_bundle_xch(
        sdk=_Sdk,
        input_spend_bundle=_InputSpendBundle(),
        requested_payments_xch=[_NotarizedPayment()],
    )
    assert result == "rebuilt-spend-bundle"
    assert calls["signer"] == (b"input-bytes", [(b"\x22" * 32, [(b"\x11" * 32, 7)])])
    assert calls["from_bytes"] == b"result-bytes"
