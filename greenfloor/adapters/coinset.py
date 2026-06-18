"""Coinset adapter: HTTP reads in Python, mutations via greenfloor-engine CLI."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.coinset_cli_mutate import (
    conservative_fee_estimate_cli,
    fee_estimate_cli,
    push_tx_cli,
)
from greenfloor.adapters.coinset_read import (
    CoinsetReadClient,
    extract_coin_ids_from_offer_payload,
    extract_coinset_tx_ids_from_offer_payload,
)

__all__ = [
    "CoinsetAdapter",
    "build_webhook_callback_url",
    "extract_coin_ids_from_offer_payload",
    "extract_coinset_tx_ids_from_offer_payload",
]


class CoinsetAdapter(CoinsetReadClient):
    def _post_json(self, endpoint: str, body: dict[str, Any]) -> dict[str, Any]:
        return self.post_json(endpoint, body)

    def push_tx(self, *, spend_bundle_hex: str) -> dict[str, Any]:
        payload = push_tx_cli(self.network, self.base_url, spend_bundle_hex)
        if not isinstance(payload, dict):
            raise RuntimeError("coinset_push_tx_invalid_response")
        return payload

    def push_tx_structured(self, *, spend_bundle: dict[str, Any]) -> dict[str, Any]:
        """Test-only fallback when a Coinset endpoint rejects hex-encoded bundles."""
        payload = self.post_json("push_tx", {"spend_bundle": spend_bundle})
        if not isinstance(payload, dict):
            return {"success": False, "error": "invalid_response_payload"}
        return payload

    def get_fee_estimate(
        self,
        *,
        target_times: list[int] | None = None,
        cost: int = 1_000_000,
        spend_count: int | None = None,
    ) -> dict[str, Any]:
        resolved_target_times = target_times or [60, 300, 600]
        spend_count_opt = (
            int(spend_count) if spend_count is not None and int(spend_count) > 0 else None
        )
        payload = fee_estimate_cli(
            self.network,
            self.base_url,
            [int(value) for value in resolved_target_times],
            int(cost),
            spend_count_opt,
        )
        if not isinstance(payload, dict):
            raise RuntimeError("coinset_get_fee_estimate_invalid_response")
        return payload

    def get_conservative_fee_estimate(
        self,
        *,
        cost: int = 1_000_000,
        spend_count: int | None = None,
    ) -> int | None:
        spend_count_opt = (
            int(spend_count) if spend_count is not None and int(spend_count) > 0 else None
        )
        return conservative_fee_estimate_cli(
            self.network,
            self.base_url,
            int(cost),
            spend_count_opt,
        )


def build_webhook_callback_url(listen_addr: str, path: str = "/coinset/tx-block") -> str:
    host, _, port = listen_addr.partition(":")
    if not port:
        port = "8787"
    return f"http://{host}:{port}{path}"
