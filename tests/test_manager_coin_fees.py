from __future__ import annotations

import json
from pathlib import Path

import greenfloor.cli.coin_ops as coin_ops_mod
from greenfloor.cli.coin_ops import coin_combine, coin_split
from greenfloor.runtime.coinset_runtime import (
    CoinsetFeeLookupPreflightError,
    _resolve_taker_or_coin_operation_fee as resolve_taker_or_coin_operation_fee,
)
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program_with_cloud_wallet,
    write_markets,
)


def test_resolve_taker_or_coin_operation_fee_uses_coinset_value(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [5, 15]}

        @staticmethod
        def get_conservative_fee_estimate():
            return 15

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    fee, source = resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=0,
    )
    assert fee == 15
    assert source == "coinset_conservative"


def test_resolve_taker_or_coin_operation_fee_applies_minimum_floor(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [2]}

        @staticmethod
        def get_conservative_fee_estimate():
            return 2

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    fee, source = resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=5,
    )
    assert fee == 5
    assert source == "coinset_conservative_minimum_floor"


def test_resolve_taker_or_coin_operation_fee_falls_back_to_config_minimum(monkeypatch) -> None:
    class _FakeCoinset:
        _calls = 0

        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [0]}

        @classmethod
        def get_conservative_fee_estimate(cls):
            cls._calls += 1
            if cls._calls == 1:
                return 1
            return None

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    monkeypatch.setattr("time.sleep", lambda _seconds: None)

    fee, source = resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=0,
    )
    assert fee == 0
    assert source == "config_minimum_fee_fallback"


def test_resolve_taker_or_coin_operation_fee_fails_on_endpoint_preflight(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            raise RuntimeError("coinset_network_error:timed_out")

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    try:
        resolve_taker_or_coin_operation_fee(network="mainnet", minimum_fee_mojos=0)
    except CoinsetFeeLookupPreflightError as exc:
        assert exc.failure_kind == "endpoint_validation_failed"
        assert "coinset_network_error" in exc.detail
    else:
        raise AssertionError("expected _CoinsetFeeLookupPreflightError")


def test_resolve_taker_or_coin_operation_fee_fails_on_temporary_advice_unavailable(
    monkeypatch,
) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": False, "error": "backend_overloaded"}

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    try:
        resolve_taker_or_coin_operation_fee(network="mainnet", minimum_fee_mojos=0)
    except CoinsetFeeLookupPreflightError as exc:
        assert exc.failure_kind == "temporary_fee_advice_unavailable"
        assert "backend_overloaded" in exc.detail
    else:
        raise AssertionError("expected _CoinsetFeeLookupPreflightError")


def test_effective_coin_split_fee_for_cat_keeps_default_fee() -> None:
    fee, source = coin_ops_mod.effective_coin_split_fee_for_asset(
        canonical_asset_id="a1",
        resolved_asset_id="Asset_cat_a1",
        fee_mojos=42,
        fee_source="coinset_conservative",
    )
    assert fee == 42
    assert source == "coinset_conservative"


def test_effective_coin_split_fee_for_xch_keeps_default_fee() -> None:
    fee, source = coin_ops_mod.effective_coin_split_fee_for_asset(
        canonical_asset_id="xch",
        resolved_asset_id="Asset_xch",
        fee_mojos=42,
        fee_source="coinset_conservative",
    )
    assert fee == 42
    assert source == "coinset_conservative"


def test_resolve_maker_offer_fee_is_zero() -> None:
    from greenfloor.runtime.coinset_runtime import resolve_maker_offer_fee

    fee, source = resolve_maker_offer_fee(network="mainnet")
    assert fee == 0
    assert source == "maker_default_zero"


def test_coin_split_no_wait_uses_advised_fee(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.runtime.cloud_wallet.adapter.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.assets.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    code = coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-1"],
        amount_per_coin=10,
        number_of_coins=2,
        no_wait=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_abc123"], 10, 2, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] is None
    assert payload["waited"] is False
    assert payload["fee_mojos"] == 42
    assert payload["fee_source"] == "coinset_conservative"
    assert payload["coin_selection_mode"] == "explicit"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_combine_no_wait_uses_advised_fee(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-2", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.runtime.cloud_wallet.adapter.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.assets.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_huun64oh7dbt9f1f9ie8khuw",
    )
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (77, "coinset_conservative"),
    )
    code = coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=3,
        asset_id="xch",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 0
    assert calls["combine"] == (3, 77, True, "Asset_huun64oh7dbt9f1f9ie8khuw", None)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] is None
    assert payload["waited"] is False
    assert payload["fee_mojos"] == 77
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["asset_id"] == "xch"
    assert payload["resolved_asset_id"] == "Asset_huun64oh7dbt9f1f9ie8khuw"


def test_coin_split_returns_structured_error_when_fee_resolution_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

    monkeypatch.setattr("greenfloor.runtime.cloud_wallet.adapter.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            RuntimeError("coinset_unavailable")
        ),
    )
    code = coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-1"],
        amount_per_coin=10,
        number_of_coins=2,
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"].startswith("fee_resolution_failed:")
    assert "coin_ops.minimum_fee_mojos" in payload["operator_guidance"]


def test_coin_combine_returns_structured_error_when_fee_resolution_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return []

    monkeypatch.setattr("greenfloor.runtime.cloud_wallet.adapter.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            RuntimeError("coinset_unavailable")
        ),
    )
    code = coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=3,
        asset_id="xch",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"].startswith("fee_resolution_failed:")
    assert "coin_ops.minimum_fee_mojos" in payload["operator_guidance"]


def test_coin_combine_distinguishes_temporary_fee_advice_unavailability(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

    monkeypatch.setattr("greenfloor.runtime.cloud_wallet.adapter.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            coin_ops_mod.CoinsetFeeLookupPreflightError(
                failure_kind="temporary_fee_advice_unavailable",
                detail="backend_overloaded",
                diagnostics={
                    "coinset_network": "mainnet",
                    "coinset_base_url": "https://coinset.org",
                },
            )
        ),
    )
    code = coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=3,
        asset_id="xch",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coinset_fee_preflight_failed:temporary_fee_advice_unavailable"
    assert payload["coinset_fee_lookup"]["failure_kind"] == "temporary_fee_advice_unavailable"
    assert "temporarily unavailable" in payload["operator_guidance"]
