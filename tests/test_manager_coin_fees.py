from __future__ import annotations

import json
from pathlib import Path

from greenfloor.cli.coin_ops_combine import coin_combine
from greenfloor.cli.coin_ops_split import coin_split
from greenfloor.runtime.coinset_runtime import (
    CoinsetFeeLookupPreflightError,
)
from greenfloor.runtime.coinset_runtime import (
    _resolve_taker_or_coin_operation_fee as resolve_taker_or_coin_operation_fee,
)
from tests.helpers.signer_coin_op_cli_fixtures import patch_signer_coin_op_cli_backend
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program_with_signer,
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


def test_resolve_maker_offer_fee_is_zero() -> None:
    from greenfloor.runtime.coinset_runtime import resolve_maker_offer_fee

    fee, source = resolve_maker_offer_fee(network="mainnet")
    assert fee == 0
    assert source == "maker_default_zero"


def test_coin_split_no_wait_uses_advised_fee(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
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

    patch_signer_coin_op_cli_backend(
        monkeypatch,
        wallet_factory=_FakeWallet,
        resolved_asset_id="Asset_split_base",
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
    assert calls["split"] == (["Coin_abc123"], 10, 2, 0)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] is None
    assert payload["waited"] is False
    assert payload["fee_mojos"] == 0
    assert payload["fee_source"] == "signer_vault_no_fee"
    assert payload["coin_selection_mode"] == "explicit"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_combine_no_wait_uses_advised_fee(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
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

    patch_signer_coin_op_cli_backend(
        monkeypatch,
        wallet_factory=_FakeWallet,
        resolved_asset_id="Asset_split_base",
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
    assert calls["combine"] == (3, 0, True, "xch", None)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] is None
    assert payload["waited"] is False
    assert payload["fee_mojos"] == 0
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["asset_id"] == "xch"
    assert payload["resolved_asset_id"] == "xch"


def test_coin_split_returns_structured_error_when_fee_resolution_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

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
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

    patch_signer_coin_op_cli_backend(
        monkeypatch,
        wallet_factory=_FakeWallet,
        resolved_asset_id="Asset_split_base",
    )
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
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["fee_mojos"] == 0
    assert payload["fee_source"] == "signer_vault_no_fee"


def test_coin_combine_returns_structured_error_when_fee_resolution_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [
                {
                    "id": "Coin_a",
                    "name": "coin-a",
                    "amount": 1000,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_split_base"},
                },
                {
                    "id": "Coin_b",
                    "name": "coin-b",
                    "amount": 1000,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_split_base"},
                },
            ]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            _ = number_of_coins, fee, largest_first, asset_id, input_coin_ids
            return {"signature_request_id": "sr-2", "status": "UNSIGNED"}

    patch_signer_coin_op_cli_backend(
        monkeypatch,
        wallet_factory=_FakeWallet,
        resolved_asset_id="Asset_split_base",
    )
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
        number_of_coins=2,
        asset_id="a1",
        coin_ids=["coin-a", "coin-b"],
        no_wait=True,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["fee_mojos"] == 0
    assert payload["fee_source"] == "signer_vault_no_fee"


def test_coin_combine_distinguishes_temporary_fee_advice_unavailability(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [
                {
                    "id": "Coin_a",
                    "name": "coin-a",
                    "amount": 1000,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_split_base"},
                },
                {
                    "id": "Coin_b",
                    "name": "coin-b",
                    "amount": 1000,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_split_base"},
                },
            ]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            _ = number_of_coins, fee, largest_first, asset_id, input_coin_ids
            return {"signature_request_id": "sr-2", "status": "UNSIGNED"}

    patch_signer_coin_op_cli_backend(
        monkeypatch,
        wallet_factory=_FakeWallet,
        resolved_asset_id="Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            CoinsetFeeLookupPreflightError(
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
        number_of_coins=2,
        asset_id="a1",
        coin_ids=["coin-a", "coin-b"],
        no_wait=True,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["fee_mojos"] == 0
    assert payload["fee_source"] == "signer_vault_no_fee"
