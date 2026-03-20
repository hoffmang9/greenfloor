"""Per-cycle Cloud Wallet ``list_coins`` cache for parallel market processing."""

from __future__ import annotations

import threading
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter


class CloudWalletAssetScopedListCache:
    """Per-daemon-cycle cache of Cloud Wallet asset-scoped ``list_coins`` results.

    Within one ``run_once`` cycle, the same resolved asset id is fetched at most
    once (thread-safe for parallel markets). Coin split/combine paths still call
    ``list_coins`` directly so they see fresh spendable state.
    """

    def __init__(self, wallet: CloudWalletAdapter) -> None:
        self._wallet = wallet
        self._lock = threading.Lock()
        self._by_asset: dict[str, list[dict[str, Any]]] = {}

    def list_coins_scoped(self, *, resolved_asset_id: str) -> list[dict[str, Any]]:
        key = str(resolved_asset_id).strip().lower()
        if not key:
            return []
        with self._lock:
            if key not in self._by_asset:
                self._by_asset[key] = self._wallet.list_coins(asset_id=resolved_asset_id)
            return self._by_asset[key]
