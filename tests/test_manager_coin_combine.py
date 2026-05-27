from __future__ import annotations

import json
from pathlib import Path

from greenfloor.cli.coin_ops import coin_combine
from tests.helpers.signer_coin_op_cli_fixtures import patch_signer_coin_op_cli_backend
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program_with_signer,
    write_markets,
    write_markets_with_ladder,
)


def test_coin_combine_with_coin_ids_resolves_to_global_ids(
    monkeypatch, tmp_path: Path, capsys
) -> None:
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
            return [
                {"id": "Coin_a", "name": "coin-a"},
                {"id": "Coin_b", "name": "coin-b"},
            ]

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
        lambda *, network, minimum_fee_mojos=0: (7, "coinset_conservative"),
    )
    code = coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=2,
        asset_id="xch",
        coin_ids=["coin-a", "coin-b"],
        no_wait=True,
    )
    assert code == 0
    assert calls["combine"] == (2, 0, True, "xch", ["Coin_a", "Coin_b"])
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["waited"] is False


def test_coin_combine_returns_structured_error_when_coin_id_not_found(
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
            return [{"id": "Coin_known", "name": "known-coin-name"}]

    patch_signer_coin_op_cli_backend(
        monkeypatch,
        wallet_factory=_FakeWallet,
        resolved_asset_id="Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    code = coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=1 + 1,
        asset_id="xch",
        coin_ids=["missing-coin-name", "known-coin-name"],
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coin_id_resolution_failed"
    assert payload["unknown_coin_ids"] == ["missing-coin-name"]


def test_coin_combine_rejects_mixed_asset_coin_ids_before_api_call(
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
                {"id": "Coin_xch", "name": "coin-xch", "asset": {"id": "xch"}},
                {"id": "Coin_cat", "name": "coin-cat", "asset": {"id": "Asset_cat"}},
            ]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            _ = number_of_coins, fee, largest_first, asset_id, input_coin_ids
            raise AssertionError("combine_coins should not be called for mixed assets")

    patch_signer_coin_op_cli_backend(
        monkeypatch,
        wallet_factory=_FakeWallet,
        resolved_asset_id="Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    code = coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=2,
        asset_id="xch",
        coin_ids=["coin-xch", "coin-cat"],
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coin_id_asset_mismatch"
    assert payload["resolved_asset_id"] == "xch"
    assert payload["mismatched_coin_ids"] == ["Coin_cat"]
    assert payload["mismatched_coin_assets"] == [
        {"coin_id": "Coin_cat", "coin_asset_id": "asset_cat"}
    ]


def test_coin_combine_uses_market_ladder_threshold_when_size_is_provided(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path, provider="splash")
    write_markets_with_ladder(markets)
    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id in {"Asset_split_base", "Asset_split_base"}:
                return [
                    {"id": f"Coin_{i}", "name": f"coin-{i}", "amount": 1500 + i, "state": "SETTLED"}
                    for i in range(6)
                ] + [
                    {"id": "Coin_dust_1", "name": "dust-1", "amount": 100, "state": "SETTLED"},
                    {"id": "Coin_dust_2", "name": "dust-2", "amount": 999, "state": "SETTLED"},
                ]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

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
        number_of_coins=0,
        asset_id=None,
        coin_ids=[],
        no_wait=True,
        venue="splash",
        size_base_units=10,
    )
    assert code == 0
    assert calls["combine"] == (
        6,
        0,
        True,
        "Asset_split_base",
        ["Coin_5", "Coin_4", "Coin_3", "Coin_2", "Coin_1", "Coin_0"],
    )
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload.get("venue") in {None, "splash"}
    assert payload["denomination_target"]["combine_threshold_count"] == 6


def test_coin_combine_ladder_threshold_uses_ceil(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path, provider="dexie")
    markets.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                '    base_asset: "a1"',
                '    base_symbol: "A1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "k1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "    pricing:",
                "      min_price_quote_per_base: 0.0031",
                "      max_price_quote_per_base: 0.0038",
                "    ladders:",
                "      sell:",
                "        - size_base_units: 10",
                "          target_count: 3",
                "          split_buffer_count: 1",
                "          combine_when_excess_factor: 1.5",
            ]
        ),
        encoding="utf-8",
    )
    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id in {"Asset_split_base", "Asset_split_base"}:
                return [
                    {"id": f"Coin_{i}", "name": f"coin-{i}", "amount": 2000 + i, "state": "SETTLED"}
                    for i in range(5)
                ]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

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
        number_of_coins=0,
        asset_id=None,
        coin_ids=[],
        no_wait=True,
        size_base_units=10,
    )
    assert code == 0
    assert calls["combine"][0] == 5


def test_coin_combine_auto_selection_ignores_cat_dust_under_one_unit(
    monkeypatch, tmp_path: Path, capsys
) -> None:
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
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [
                    {"id": "Coin_big_1", "name": "big-1", "amount": 2000, "state": "SETTLED"},
                    {"id": "Coin_big_2", "name": "big-2", "amount": 1500, "state": "SETTLED"},
                    {"id": "Coin_dust_1", "name": "dust-1", "amount": 999, "state": "SETTLED"},
                    {"id": "Coin_dust_2", "name": "dust-2", "amount": 100, "state": "SETTLED"},
                ]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-combine", "status": "UNSIGNED"}

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
        number_of_coins=2,
        asset_id="Asset_split_base",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 0
    assert calls["combine"] == (
        2,
        0,
        True,
        "Asset_split_base",
        ["Coin_big_1", "Coin_big_2"],
    )
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_combine_auto_selection_directly_filters_cross_asset_scoped_rows(
    monkeypatch, tmp_path: Path, capsys
) -> None:
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
            _ = include_pending
            if asset_id in {"a1", "Asset_split_base"}:
                return [
                    {
                        "id": "Coin_good_1",
                        "name": "good-1",
                        "amount": 1000,
                        "state": "SETTLED",
                        "asset": {"id": "Asset_split_base"},
                    },
                    {
                        "id": "Coin_bad",
                        "name": "bad",
                        "amount": 1000,
                        "state": "LOCKED",
                        "asset": {"id": "Asset_huun64oh7dbt9f1f9ie8khuw"},
                    },
                    {
                        "id": "Coin_good_2",
                        "name": "good-2",
                        "amount": 1000,
                        "state": "SETTLED",
                        "asset": {"id": "Asset_split_base"},
                    },
                ]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def get_coin_record(*, coin_id):
            mapping = {
                "Coin_good_1": {
                    "id": "Coin_good_1",
                    "amount": 1000,
                    "state": "SETTLED",
                    "isLocked": False,
                    "isLinkedToOpenOffer": False,
                    "asset": {"id": "Asset_split_base"},
                },
                "Coin_good_2": {
                    "id": "Coin_good_2",
                    "amount": 1000,
                    "state": "SETTLED",
                    "isLocked": False,
                    "isLinkedToOpenOffer": False,
                    "asset": {"id": "Asset_split_base"},
                },
                "Coin_bad": {
                    "id": "Coin_bad",
                    "amount": 1000,
                    "state": "SETTLED",
                    "isLocked": False,
                    "isLinkedToOpenOffer": False,
                    "asset": {"id": "Asset_huun64oh7dbt9f1f9ie8khuw"},
                },
            }
            return mapping[coin_id]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-combine", "status": "UNSIGNED"}

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
        number_of_coins=2,
        asset_id="Asset_split_base",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 0
    assert calls["combine"] == (
        2,
        0,
        True,
        "Asset_split_base",
        ["Coin_good_1", "Coin_good_2"],
    )
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_selection_mode"] == "adapter_auto_select"


def test_coin_combine_until_ready_with_coin_ids_stops_with_requires_new_coin_selection(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path, provider="dexie")
    write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            # 8 coins > combine_threshold=6 → not ready after combine
            return [
                {
                    "id": f"Coin_{i}",
                    "name": f"coin-{i}",
                    "amount": 10000,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_split_base"},
                }
                for i in range(8)
            ]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            return {"signature_request_id": "sr-combine", "status": "SIGNED"}

    patch_signer_coin_op_cli_backend(
        monkeypatch,
        wallet_factory=_FakeWallet,
        resolved_asset_id="Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )

    # Provide explicit coin IDs so loop cannot auto-select new candidates
    code = coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=6,  # matches combine_threshold and len(coin_ids)
        asset_id="Asset_split_base",
        coin_ids=[f"coin-{i}" for i in range(6)],
        no_wait=False,
        size_base_units=10,
        until_ready=True,
        max_iterations=3,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["stop_reason"] in {"requires_new_coin_selection", "single_pass", "ready"}
    assert payload["denomination_readiness"]["ready"] is True
