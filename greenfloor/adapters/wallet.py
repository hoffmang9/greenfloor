from __future__ import annotations

import json
import os

from greenfloor.runtime.coinset_coins import list_unspent_coins_by_receive_address


class WalletAdapter:
    def list_asset_coins_base_units(
        self,
        *,
        asset_id: str,
        key_id: str,
        receive_address: str,
        network: str,
    ) -> list[int]:
        _ = key_id
        raw = os.getenv("GREENFLOOR_FAKE_COINS_JSON", "").strip()
        if raw:
            fake = self._list_fake_coin_amounts(raw=raw, asset_id=asset_id)
            if fake:
                return fake

        if not self._is_xch_asset(asset_id):
            cat_raw = os.getenv("GREENFLOOR_FAKE_CAT_COINS_JSON", "").strip()
            if cat_raw:
                return self._list_fake_coin_amounts(raw=cat_raw, asset_id=asset_id)
            try:
                coins = list_unspent_coins_by_receive_address(
                    network=str(network).strip(),
                    receive_address=str(receive_address).strip(),
                    asset_id=str(asset_id).strip(),
                )
            except Exception:
                return []
            return [int(coin["amount"]) for coin in coins if int(coin.get("amount", 0)) > 0]

        return self._list_coin_amounts_via_engine(
            receive_address=receive_address,
            network=network,
        )

    @staticmethod
    def _is_xch_asset(asset_id: str) -> bool:
        lowered = asset_id.strip().lower()
        return lowered in {"xch", "1", ""}

    def _list_fake_coin_amounts(self, *, raw: str, asset_id: str) -> list[int]:
        try:
            data = json.loads(raw)
        except json.JSONDecodeError:
            return []
        if not isinstance(data, dict):
            return []
        values = data.get(asset_id, [])
        if not isinstance(values, list):
            return []
        out: list[int] = []
        for value in values:
            try:
                out.append(int(value))
            except (TypeError, ValueError):
                continue
        return out

    def _list_coin_amounts_via_engine(
        self,
        *,
        receive_address: str,
        network: str,
    ) -> list[int]:
        try:
            coins = list_unspent_coins_by_receive_address(
                network=str(network).strip(),
                receive_address=str(receive_address).strip(),
                asset_id="xch",
            )
        except Exception:
            return []
        return [int(coin["amount"]) for coin in coins if int(coin.get("amount", 0)) > 0]
