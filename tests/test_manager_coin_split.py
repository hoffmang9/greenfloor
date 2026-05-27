from __future__ import annotations

import json
from pathlib import Path

from greenfloor.cli.coin_ops_split import coin_split
from greenfloor.runtime.coinset_runtime import CoinsetFeeLookupPreflightError
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program,
    write_manager_program_with_cloud_wallet,
    write_markets,
    write_markets_with_ladder,
)


def test_coin_split_auto_selects_largest_spendable_asset_coin(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    calls: dict[str, tuple[list[str], int, int, int] | None] = {"split": None}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [
                    {"id": "Coin_small", "name": "small", "amount": 100, "state": "SETTLED"},
                    {"id": "Coin_big", "name": "big", "amount": 1500, "state": "SETTLED"},
                    {"id": "Coin_reserve", "name": "reserve", "amount": 1100, "state": "SETTLED"},
                    {"id": "Coin_pending", "name": "pending", "amount": 999, "state": "PENDING"},
                ]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-auto", "status": "UNSIGNED"}

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
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_big"], 10, 10, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_split_guardrail_blocks_when_it_would_lock_all_spendable_coins(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    split_called = [False]

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [{"id": "Coin_only", "name": "only", "amount": 1500, "state": "SETTLED"}]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            split_called[0] = True
            return {"signature_request_id": "sr-guard", "status": "UNSIGNED"}

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
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
    )
    assert code == 2
    assert split_called[0] is False
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["error"] == "coin_split_guardrail_would_lock_all_spendable_coins"
    assert payload["spendable_asset_coin_count"] == 1
    assert payload["selected_spendable_coin_count"] == 1


def test_coin_split_guardrail_override_allows_lock_all_spendable(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    calls: dict[str, tuple[list[str], int, int, int] | None] = {"split": None}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [{"id": "Coin_only", "name": "only", "amount": 1500, "state": "SETTLED"}]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-override", "status": "UNSIGNED"}

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
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
        allow_lock_all_spendable=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_only"], 10, 10, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_split_guardrail_prompt_override_allows_continue(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    calls: dict[str, tuple[list[str], int, int, int] | None] = {"split": None}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [{"id": "Coin_only", "name": "only", "amount": 1500, "state": "SETTLED"}]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-prompt", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.runtime.cloud_wallet.adapter.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.assets.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    monkeypatch.setattr("builtins.input", lambda _prompt: "y")

    code = coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
        prompt_for_override=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_only"], 10, 10, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_split_distinguishes_coinset_endpoint_preflight_failure(
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
            CoinsetFeeLookupPreflightError(
                failure_kind="endpoint_validation_failed",
                detail="coinset_network_error:timed_out",
                diagnostics={
                    "coinset_network": "mainnet",
                    "coinset_base_url": "https://coinset.org",
                },
            )
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
    assert payload["error"] == "coinset_fee_preflight_failed:endpoint_validation_failed"
    assert payload["coinset_fee_lookup"]["failure_kind"] == "endpoint_validation_failed"
    assert "endpoint routing" in payload["operator_guidance"]


def test_coin_split_returns_structured_error_when_coin_id_not_found(
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
            return [{"id": "Coin_known", "name": "known-coin-name"}]

    monkeypatch.setattr("greenfloor.runtime.cloud_wallet.adapter.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    code = coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["missing-coin-name"],
        amount_per_coin=10,
        number_of_coins=2,
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coin_id_resolution_failed"
    assert payload["unknown_coin_ids"] == ["missing-coin-name"]


def test_coin_split_uses_market_ladder_target_when_size_is_provided(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path, provider="splash")
    write_markets_with_ladder(markets)
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
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=True,
        venue="splash",
        size_base_units=10,
    )
    assert code == 0
    assert calls["split"] == (["Coin_abc123"], 10, 4, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] == "splash"
    assert payload["denomination_target"]["required_count"] == 4


def test_coin_split_until_ready_ignores_unknown_states_and_string_asset(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path, provider="dexie")
    write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"
        _calls = 0

        def __init__(self, _config):
            pass

        @classmethod
        def list_coins(cls, *, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            cls._calls += 1
            return [
                {"id": "Coin_a", "name": "coin-a", "amount": 10, "state": "LOCKED", "asset": "a1"},
                {
                    "id": "Coin_b",
                    "name": "coin-b",
                    "amount": 10,
                    "state": "MYSTERY",
                    "asset": {"id": "a1"},
                },
            ]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            return {"signature_request_id": "sr-1", "status": "SIGNED"}

        @staticmethod
        def get_signature_request(signature_request_id):
            _ = signature_request_id
            return {"status": "SIGNED"}

    monkeypatch.setattr("greenfloor.runtime.cloud_wallet.adapter.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling.wait_for_mempool_then_confirmation",
        lambda **kwargs: [],
    )

    code = coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=False,
        size_base_units=10,
        until_ready=True,
        max_iterations=1,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["error"] == "no_spendable_split_coin_available"
    assert payload["resolved_asset_id"] == "a1"


def test_coin_split_until_ready_requires_size_base_units(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)
    try:
        coin_split(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id="m1",
            pair=None,
            coin_ids=[],
            amount_per_coin=10,
            number_of_coins=2,
            no_wait=False,
            until_ready=True,
            size_base_units=None,
        )
    except ValueError as exc:
        assert "--size-base-units" in str(exc)
    else:
        raise AssertionError("expected ValueError")


def test_coin_split_until_ready_disallows_no_wait(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets_with_ladder(markets)
    try:
        coin_split(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id="m1",
            pair=None,
            coin_ids=[],
            amount_per_coin=10,
            number_of_coins=4,
            no_wait=True,
            until_ready=True,
            size_base_units=10,
        )
    except ValueError as exc:
        assert "requires wait mode" in str(exc)
    else:
        raise AssertionError("expected ValueError")


def test_coin_split_until_ready_reports_not_ready(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path, provider="dexie")
    write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"
        _calls = 0

        def __init__(self, _config):
            pass

        @classmethod
        def list_coins(cls, *, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            cls._calls += 1
            # Never reaches target 4 coins of size 10.
            return [
                {
                    "id": "Coin_a",
                    "name": "coin-a",
                    "amount": 10,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                },
                {
                    "id": "Coin_b",
                    "name": "coin-b",
                    "amount": 9,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                },
            ]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            return {"signature_request_id": "sr-1", "status": "SIGNED"}

        @staticmethod
        def get_signature_request(signature_request_id):
            _ = signature_request_id
            return {"status": "SIGNED"}

    monkeypatch.setattr("greenfloor.runtime.cloud_wallet.adapter.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling.poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SIGNED", []),
    )
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling.wait_for_mempool_then_confirmation",
        lambda **kwargs: [],
    )

    code = coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-a"],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=False,
        venue="dexie",
        size_base_units=10,
        until_ready=True,
        max_iterations=2,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["until_ready"] is True
    assert payload["stop_reason"] == "requires_new_coin_selection"
    assert payload["denomination_readiness"]["ready"] is False
    assert len(payload["operations"]) == 1


def test_coin_split_until_ready_succeeds_when_denominations_met(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path, provider="dexie")
    write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            # 4 confirmed coins of size 10 + one larger reserve coin for asset a1.
            rows = [
                {
                    "id": f"Coin_{i}",
                    "name": f"coin-{i}",
                    "amount": 10,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
                for i in range(4)
            ]
            rows.append(
                {
                    "id": "Coin_reserve",
                    "name": "coin-reserve",
                    "amount": 20,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
            )
            return rows

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            return {"signature_request_id": "sr-ok", "status": "SIGNED"}

    monkeypatch.setattr("greenfloor.runtime.cloud_wallet.adapter.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling.poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SIGNED", []),
    )
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling.wait_for_mempool_then_confirmation",
        lambda **kwargs: [],
    )

    code = coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=False,
        size_base_units=10,
        until_ready=True,
        max_iterations=3,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["stop_reason"] == "ready"
    assert payload["denomination_readiness"]["ready"] is True
    assert payload["split_gate"]["reserve_ready"] is True


def test_coin_split_gate_ready_skips_split_in_non_interactive_mode(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path, provider="dexie")
    write_markets_with_ladder(markets)

    split_called = [False]

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [
                {
                    "id": f"Coin_{i}",
                    "name": f"coin-{i}",
                    "amount": 10,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
                for i in range(4)
            ] + [
                {
                    "id": "Coin_reserve",
                    "name": "coin-reserve",
                    "amount": 50,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
            ]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            split_called[0] = True
            return {"signature_request_id": "sr-should-not-run", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.runtime.cloud_wallet.adapter.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )

    code = coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=True,
        size_base_units=10,
        until_ready=False,
        max_iterations=1,
        prompt_for_override=False,
    )
    assert code == 0
    assert split_called[0] is False
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["stop_reason"] == "ready"
    assert payload["split_gate"]["ready"] is True
