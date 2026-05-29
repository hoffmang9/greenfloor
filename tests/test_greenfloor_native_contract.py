from __future__ import annotations

from greenfloor.core.offer_policy import verify_offer_for_dexie
from tests.helpers.kernel_mock import install_kernel_stub


def test_verify_offer_for_dexie_uses_greenfloor_signer_only(monkeypatch) -> None:
    calls: dict[str, str] = {}

    class _Signer:
        @staticmethod
        def verify_offer_for_dexie(offer: str) -> None:
            calls["offer"] = offer

    install_kernel_stub(monkeypatch, _Signer)

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
