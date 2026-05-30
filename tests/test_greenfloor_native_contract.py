from __future__ import annotations

from greenfloor.core.policy_bridge import verify_offer_for_dexie
from tests.helpers.engine_mock import install_engine_stub


def test_verify_offer_for_dexie_uses_greenfloor_engine_only(monkeypatch) -> None:
    calls: dict[str, str] = {}

    class _Signer:
        @staticmethod
        def verify_offer_for_dexie(offer: str) -> None:
            calls["offer"] = offer

    install_engine_stub(monkeypatch, _Signer)

    assert verify_offer_for_dexie("offer1contract") is None
    assert calls["offer"] == "offer1contract"


def test_verify_offer_for_dexie_reports_missing_engine(monkeypatch) -> None:
    monkeypatch.setattr(
        "greenfloor.core.engine_bridge.import_engine",
        lambda: (_ for _ in ()).throw(ImportError("disable signer path for this test")),
    )

    assert verify_offer_for_dexie("offer1contract") == (
        "greenfloor_engine_unavailable:verify_offer_for_dexie"
    )
