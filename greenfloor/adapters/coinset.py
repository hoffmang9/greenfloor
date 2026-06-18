"""Coinset adapter: all IO via greenfloor-engine CLI (post for reads, subcommands for mutations)."""

from __future__ import annotations

from greenfloor.adapters.coinset_engine import (
    CoinsetReadClient,
    conservative_fee_estimate_cli,
    extract_coin_ids_from_offer_payload,
    extract_coinset_tx_ids_from_offer_payload,
    fee_estimate_cli,
    push_tx_cli,
)

__all__ = [
    "CoinsetAdapter",
    "build_webhook_callback_url",
    "extract_coin_ids_from_offer_payload",
    "extract_coinset_tx_ids_from_offer_payload",
]


class CoinsetAdapter(CoinsetReadClient):
    def push_tx(self, *, spend_bundle_hex: str) -> dict[str, object]:
        payload = push_tx_cli(self.network, self.base_url, spend_bundle_hex)
        if not isinstance(payload, dict):
            raise RuntimeError("coinset_push_tx_invalid_response")
        return payload

    def get_fee_estimate(
        self,
        *,
        target_times: list[int] | None = None,
        cost: int = 1_000_000,
        spend_count: int | None = None,
    ) -> dict[str, object]:
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
