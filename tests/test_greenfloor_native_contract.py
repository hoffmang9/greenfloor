from __future__ import annotations

import sys

import greenfloor.adapters.native_offer as native_offer_mod
from greenfloor.runtime.offer_publish import (
    verify_offer_text_for_dexie as _verify_offer_text_for_dexie,
)


def test_verify_offer_text_for_dexie_uses_greenfloor_signer_only(monkeypatch) -> None:
    calls: dict[str, str] = {}

    class _Signer:
        @staticmethod
        def verify_offer_for_dexie(offer: str) -> None:
            calls["offer"] = offer

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Signer)

    assert _verify_offer_text_for_dexie("offer1contract") is None
    assert calls["offer"] == "offer1contract"


def test_verify_offer_text_for_dexie_reports_missing_kernel(monkeypatch) -> None:
    import greenfloor.runtime.offer_publish as offer_publish_mod

    def _fail_verify(_offer: str) -> str | None:
        raise ImportError("disable signer path for this test")

    monkeypatch.setattr(offer_publish_mod, "verify_offer_for_dexie", _fail_verify)
    monkeypatch.delitem(sys.modules, "greenfloor_signer", raising=False)

    assert _verify_offer_text_for_dexie("offer1contract") == (
        "wallet_sdk_import_error:greenfloor_signer_unavailable"
    )


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

    monkeypatch.setattr(native_offer_mod, "import_kernel", lambda: _Signer)

    result = native_offer_mod.from_input_spend_bundle_xch(
        sdk=_Sdk,
        input_spend_bundle=_InputSpendBundle(),
        requested_payments_xch=[_NotarizedPayment()],
    )
    assert result == "rebuilt-spend-bundle"
    assert calls["signer"] == (b"input-bytes", [(b"\x22" * 32, [(b"\x11" * 32, 7)])])
    assert calls["from_bytes"] == b"result-bytes"
