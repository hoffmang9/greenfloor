"""Shared mocks for signer-backed manager coin-op CLI tests."""

from __future__ import annotations

from collections.abc import Callable
from typing import Any, cast

from greenfloor.config.models import MarketConfig
from greenfloor.runtime.coin_ops.models import CoinOpSelectionMode
from greenfloor.runtime.coin_ops_scope import CoinOpScope


def write_manager_markets_home(tmp_path, write_markets_fn) -> None:
    markets_dir = tmp_path / "config"
    markets_dir.mkdir(parents=True, exist_ok=True)
    write_markets_fn(markets_dir / "markets.yaml")


def _spendable_states() -> set[str]:
    return {"SETTLED", "CONFIRMED", "SPENDABLE"}


def _instantiate_wallet(wallet_factory: Callable[[], Any] | type) -> Any:
    if isinstance(wallet_factory, type):
        try:
            return wallet_factory()
        except TypeError:
            return wallet_factory(object())  # type: ignore[misc]
    return wallet_factory()


class SignerCoinOpBackendFake:
    """Minimal CoinOpBackend stand-in wrapping a wallet test double."""

    def __init__(
        self,
        *,
        wallet: Any,
        market: MarketConfig | None = None,
        resolved_asset_id: str = "Asset_resolved",
    ) -> None:
        self._wallet = wallet
        self.resolved_asset_id = resolved_asset_id
        assert market is not None
        self.scope = CoinOpScope(
            market=market,
            selected_venue=None,
            execution_backend="signer",
            vault_id="signer",
        )

    def list_wallet_coins(self) -> list[dict[str, Any]]:
        return self.list_asset_scoped_coins()

    def list_asset_scoped_coins(self) -> list[dict[str, Any]]:
        list_coins = getattr(self._wallet, "list_coins", None)
        if callable(list_coins):
            return list(
                cast(
                    list[dict[str, Any]],
                    list_coins(include_pending=True, asset_id=self.resolved_asset_id),
                )
            )
        return []

    def filter_spendable(
        self,
        coins: list[dict[str, Any]],
        *,
        canonical_asset_id: str,
        min_coin_amount_mojos: int,
        mode: CoinOpSelectionMode,
        verify_direct_spendable_lookup: bool = False,
    ) -> list[dict[str, Any]]:
        _ = canonical_asset_id, mode, verify_direct_spendable_lookup
        return [
            coin
            for coin in coins
            if str(coin.get("state", "CONFIRMED")).upper() in _spendable_states()
            and int(coin.get("amount", 0)) >= int(min_coin_amount_mojos)
        ]

    def resolve_coin_ids(
        self, wallet_coins: list[dict[str, Any]], raw_coin_ids: list[str]
    ) -> tuple[list[str], list[str]]:
        mapping: dict[str, str] = {}
        for coin in wallet_coins:
            coin_id = str(coin.get("id", coin.get("name", ""))).strip()
            name = str(coin.get("name", coin_id)).strip()
            for token in (coin_id, name):
                normalized = token.lower().removeprefix("0x")
                if normalized:
                    mapping[normalized] = coin_id
        resolved: list[str] = []
        unresolved: list[str] = []
        for raw in raw_coin_ids:
            token = str(raw).strip().lower().removeprefix("0x")
            mapped = mapping.get(token)
            if mapped:
                resolved.append(mapped)
            else:
                unresolved.append(str(raw))
        return resolved, unresolved

    def split_coins(
        self,
        *,
        coin_ids: list[str],
        amount_per_coin: int,
        number_of_coins: int,
        fee_mojos: int,
        initial_coin_ids: set[str] | None = None,
    ) -> dict[str, Any]:
        _ = initial_coin_ids
        return self._wallet.split_coins(
            coin_ids=coin_ids,
            amount_per_coin=amount_per_coin,
            number_of_coins=number_of_coins,
            fee=fee_mojos,
        )

    def combine_coins(
        self,
        *,
        number_of_coins: int,
        fee_mojos: int,
        input_coin_ids: list[str] | None,
        largest_first: bool = True,
        target_amount: int | None = None,
    ) -> dict[str, Any]:
        asset_id = self.resolved_asset_id
        return self._wallet.combine_coins(
            number_of_coins=number_of_coins,
            fee=fee_mojos,
            largest_first=largest_first,
            asset_id=asset_id,
            input_coin_ids=input_coin_ids,
        )

    def evaluate_denomination_readiness(
        self,
        *,
        asset_id: str,
        size_base_units: int,
        required_min_count: int | None = None,
        max_allowed_count: int | None = None,
    ) -> dict[str, int | bool | str]:
        from greenfloor.core.coin_ops import (
            combine_denomination_readiness,
            split_denomination_readiness,
        )

        coins = self.list_asset_scoped_coins()
        if required_min_count is not None and max_allowed_count is None:
            return split_denomination_readiness(
                asset_scoped_coins=coins,
                asset_id=asset_id,
                size_base_units=int(size_base_units),
                required_min_count=int(required_min_count),
            )
        spendable = [
            coin
            for coin in coins
            if str(coin.get("state", "CONFIRMED")).upper() in _spendable_states()
        ]
        matching = [
            coin for coin in spendable if int(coin.get("amount", 0)) == int(size_base_units)
        ]
        if max_allowed_count is not None:
            return combine_denomination_readiness(
                asset_id=asset_id,
                size_base_units=int(size_base_units),
                max_allowed_count=int(max_allowed_count),
                matching_count=len(matching),
            )
        return {
            "asset_id": asset_id,
            "size_base_units": int(size_base_units),
            "required_min_count": -1,
            "current_count": len(matching),
            "ready": True,
        }

    def build_iteration_payload(
        self,
        *,
        operation_id: str,
        operation_state: str,
        no_wait: bool,
        network: str,
        existing_coin_ids: set[str],
        iteration: int,
        readiness_asset_id: str,
        readiness_kwargs: dict[str, int],
        denomination_target: Any,
    ) -> tuple[dict[str, object], dict[str, int | bool | str] | None]:
        _ = network, existing_coin_ids
        final_readiness = None
        if denomination_target is not None:
            final_readiness = self.evaluate_denomination_readiness(
                asset_id=readiness_asset_id,
                size_base_units=int(denomination_target.size_base_units),
                **readiness_kwargs,
            )
        payload: dict[str, object] = {
            "iteration": iteration,
            "operation_id": operation_id,
            "operation_state": operation_state,
            "signature_request_id": operation_id,
            "signature_state": operation_state,
            "waited": not no_wait,
            "wait_events": [],
        }
        if final_readiness is not None:
            payload["denomination_readiness"] = final_readiness
        return payload, final_readiness


def _patch_signer_asset_resolvers(
    monkeypatch,
    *,
    resolved_asset_id: str,
    import_paths: list[str],
) -> None:
    def _resolve(*_args: object, **_kwargs: object) -> str:
        canonical = str(_kwargs.get("canonical_asset_id", "")).strip().lower()
        if canonical in {"xch", "txch"}:
            return "xch"
        return resolved_asset_id

    for path in import_paths:
        monkeypatch.setattr(f"{path}.resolve_signer_asset_id", _resolve)
        if path == "greenfloor.runtime.coin_ops.runtime":
            monkeypatch.setattr(f"{path}.resolve_coin_op_base_asset_id", _resolve)
    monkeypatch.setattr(
        "greenfloor.config.models.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )


def patch_signer_coin_op_cli_backend(
    monkeypatch,
    *,
    wallet_factory: Callable[[], Any] | type,
    resolved_asset_id: str = "Asset_resolved",
) -> None:
    def _build_backend(**kwargs: Any) -> SignerCoinOpBackendFake:
        market = kwargs.get("market")
        asset_id = str(kwargs.get("resolved_asset_id") or resolved_asset_id)
        return SignerCoinOpBackendFake(
            wallet=_instantiate_wallet(wallet_factory),
            market=market,
            resolved_asset_id=asset_id,
        )

    monkeypatch.setattr(
        "greenfloor.runtime.coin_ops.runtime.build_coin_op_backend",
        _build_backend,
    )
    _patch_signer_asset_resolvers(
        monkeypatch,
        resolved_asset_id=resolved_asset_id,
        import_paths=["greenfloor.runtime.coin_ops.runtime"],
    )


def patch_signer_coins_list_backend(
    monkeypatch,
    *,
    wallet_factory: Callable[[], Any] | type,
    resolved_asset_id: str = "Asset_resolved",
) -> None:
    def _build_backend(**kwargs: Any) -> SignerCoinOpBackendFake:
        market = kwargs.get("market")
        asset_id = str(kwargs.get("resolved_asset_id") or resolved_asset_id)
        return SignerCoinOpBackendFake(
            wallet=_instantiate_wallet(wallet_factory),
            market=market,
            resolved_asset_id=asset_id,
        )

    monkeypatch.setattr("greenfloor.cli.coin_ops_list.build_coin_op_backend", _build_backend)
    _patch_signer_asset_resolvers(
        monkeypatch,
        resolved_asset_id=resolved_asset_id,
        import_paths=["greenfloor.cli.coin_ops_list"],
    )
