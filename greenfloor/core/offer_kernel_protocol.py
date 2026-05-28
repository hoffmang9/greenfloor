"""Offer build/validate PyO3 protocol surface."""

from __future__ import annotations

from typing import Any, Protocol


class OfferPolicyKernelProtocol(Protocol):
    def resolve_offer_expiry_for_pricing(self, pricing: dict[str, Any]) -> tuple[str, int]: ...

    def resolve_quote_price_for_pricing(self, pricing: dict[str, Any]) -> float: ...

    def mojo_multiplier_for_leg(
        self, pricing: dict[str, Any], field: str, asset_id: str
    ) -> int: ...

    def verify_offer_for_dexie(self, offer: str) -> str | None: ...
