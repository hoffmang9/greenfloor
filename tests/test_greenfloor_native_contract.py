from __future__ import annotations

import sys

from greenfloor.core.offer_policy import verify_offer_for_dexie


def test_verify_offer_for_dexie_uses_greenfloor_signer_only(monkeypatch) -> None:
    calls: dict[str, str] = {}

    class _Signer:
        @staticmethod
        def verify_offer_for_dexie(offer: str) -> None:
            calls["offer"] = offer

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Signer)

    assert verify_offer_for_dexie("offer1contract") is None
    assert calls["offer"] == "offer1contract"


def test_verify_offer_for_dexie_reports_missing_kernel(monkeypatch) -> None:
    monkeypatch.setattr(
        "greenfloor.core.kernel_bridge.import_kernel",
        lambda: (_ for _ in ()).throw(ImportError("disable signer path for this test")),
    )

    assert verify_offer_for_dexie("offer1contract") == (
        "wallet_sdk_import_error:greenfloor_signer_unavailable"
    )
