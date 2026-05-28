"""Cancel-policy PyO3 protocol surface."""

from __future__ import annotations

from typing import TYPE_CHECKING, Protocol

if TYPE_CHECKING:
    from greenfloor.core.cancel_policy import CancelPolicyDecision, OpenOfferRow


class CancelPolicyKernelProtocol(Protocol):
    def abs_move_bps(self, current: float | None, previous: float | None) -> float | None: ...

    def cancel_move_threshold_bps(
        self, market_threshold: int | None, env_threshold: int | None
    ) -> int: ...

    def evaluate_cancel_policy_decision(
        self,
        quote_asset_type: str,
        cancel_policy_stable_vs_unstable: bool,
        current_xch_price_usd: float | None,
        previous_xch_price_usd: float | None,
        market_threshold: int | None,
        env_threshold: int | None,
    ) -> CancelPolicyDecision: ...

    def collect_open_offer_ids_for_cancel(self, offers: list[OpenOfferRow]) -> list[str]: ...
