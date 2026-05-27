from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from greenfloor.cli.offers_lifecycle import offers_cancel
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program_with_cloud_wallet,
    write_markets,
)


def test_offers_cancel_cancel_open_uses_cloud_wallet(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)

    cancelled: list[tuple[str, bool]] = []

    class _FakeWallet:
        vault_id = "wallet-1"

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {
                "offers": [
                    {
                        "id": "WalletOffer_1",
                        "offerId": "Offer_1",
                        "state": "OPEN",
                        "expiresAt": "2026-02-26T01:13:22.000Z",
                    },
                    {
                        "id": "WalletOffer_2",
                        "offerId": "Offer_2",
                        "state": "EXPIRED",
                        "expiresAt": "2026-02-25T21:13:22.000Z",
                    },
                ]
            }

        @staticmethod
        def cancel_offer(*, offer_id: str, cancel_off_chain: bool = False):
            cancelled.append((offer_id, cancel_off_chain))
            return {"signature_request_id": f"SignatureRequest_{offer_id}", "status": "SUBMITTED"}

    monkeypatch.setattr(
        "greenfloor.cli.offers_lifecycle.cloud_wallet_adapter.new_cloud_wallet_adapter",
        lambda _p: _FakeWallet(),
    )

    code = offers_cancel(
        program_path=program,
        offer_ids=[],
        cancel_open=True,
    )
    assert code == 0
    assert cancelled == [("Offer_1", False)]
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["selected_count"] == 1
    assert payload["cancelled_count"] == 1
    assert payload["items"][0]["offer_id"] == "Offer_1"
    assert (
        payload["items"][0]["url"]
        == "https://wallet.example.com/wallet/wallet-1/offers/WalletOffer_1"
    )


def test_offers_cancel_pending_offer_uses_off_chain_cancel(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)

    cancelled: list[tuple[str, bool]] = []

    class _FakeWallet:
        vault_id = "wallet-1"

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {
                "offers": [
                    {
                        "id": "WalletOffer_pending",
                        "offerId": "Offer_pending",
                        "state": "PENDING",
                        "expiresAt": "2026-02-26T01:13:22.000Z",
                    }
                ]
            }

        @staticmethod
        def cancel_offer(*, offer_id: str, cancel_off_chain: bool = False):
            cancelled.append((offer_id, cancel_off_chain))
            # Off-chain cancel may not return a signature request.
            return {"signature_request_id": "", "status": ""}

    monkeypatch.setattr(
        "greenfloor.cli.offers_lifecycle.cloud_wallet_adapter.new_cloud_wallet_adapter",
        lambda _p: _FakeWallet(),
    )

    code = offers_cancel(
        program_path=program,
        offer_ids=["Offer_pending"],
        cancel_open=False,
    )
    assert code == 0
    assert cancelled == [("Offer_pending", True)]
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["selected_count"] == 1
    assert payload["cancelled_count"] == 1
    assert payload["failed_count"] == 0
    assert payload["items"][0]["cancel_off_chain"] is True
    assert payload["items"][0]["result"]["success"] is True
    assert payload["items"][0]["result"]["reason"] == "cancel_off_chain_requested"


def test_offers_cancel_can_submit_onchain_refresh_after_offchain(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    cancelled: list[tuple[str, bool]] = []
    split_calls: list[dict[str, Any]] = []

    class _Program:
        app_network = "mainnet"
        cloud_wallet_base_url = "https://wallet.example.com"
        signer_key_registry = {}
        home_dir = str(tmp_path / "gf_home")

    class _FakeWallet:
        vault_id = "wallet-1"

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {
                "offers": [
                    {
                        "id": "WalletOffer_pending",
                        "offerId": "Offer_pending",
                        "state": "PENDING",
                        "expiresAt": "2026-02-26T01:13:22.000Z",
                        "bech32": "offer1dummy",
                    }
                ]
            }

        @staticmethod
        def cancel_offer(*, offer_id: str, cancel_off_chain: bool = False):
            cancelled.append((offer_id, cancel_off_chain))
            return {"signature_request_id": "", "status": ""}

        @staticmethod
        def list_coins(*, asset_id: str | None = None, include_pending: bool = True):
            _ = asset_id, include_pending
            return [
                {
                    "id": "Coin_target",
                    "name": "ab" * 32,
                    "amount": 1000,
                    "state": "CONFIRMED",
                    "asset": {"id": "Asset_a1"},
                }
            ]

        @staticmethod
        def split_coins(
            *,
            coin_ids: list[str],
            amount_per_coin: int,
            number_of_coins: int,
            fee: int,
        ):
            split_calls.append(
                {
                    "coin_ids": coin_ids,
                    "amount_per_coin": amount_per_coin,
                    "number_of_coins": number_of_coins,
                    "fee": fee,
                }
            )
            return {"signature_request_id": "SignatureRequest_refresh", "status": "UNSIGNED"}

    monkeypatch.setattr(
        "greenfloor.cli.offers_lifecycle.load_program_config", lambda _p: _Program()
    )
    monkeypatch.setattr(
        "greenfloor.cli.offers_lifecycle.cloud_wallet_adapter.new_cloud_wallet_adapter",
        lambda _p: _FakeWallet(),
    )
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.assets.resolve_cloud_wallet_asset_id",
        lambda **_kw: "Asset_a1",
    )
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime._resolve_taker_or_coin_operation_fee",
        lambda **_kw: (0, "coinset_conservative"),
    )

    code = offers_cancel(
        program_path=program,
        offer_ids=["Offer_pending"],
        cancel_open=False,
        markets_path=markets,
        submit_onchain_after_offchain=True,
        onchain_market_id="m1",
    )
    assert code == 0
    assert cancelled == [("Offer_pending", True)]
    assert len(split_calls) == 1
    assert split_calls[0]["coin_ids"] == ["Coin_target"]
    assert split_calls[0]["amount_per_coin"] == 1000
    assert split_calls[0]["number_of_coins"] == 1
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["submit_onchain_after_offchain"] is True
    assert payload["onchain_market_id"] == "m1"
    assert payload["items"][0]["result"]["onchain_refresh"]["status"] == "executed"
    assert (
        payload["items"][0]["result"]["onchain_refresh"]["signature_request_id"]
        == "SignatureRequest_refresh"
    )


def test_offers_cancel_submit_onchain_requires_market_selection(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    class _Program:
        app_network = "mainnet"
        cloud_wallet_base_url = "https://wallet.example.com"
        signer_key_registry = {}

    monkeypatch.setattr(
        "greenfloor.cli.offers_lifecycle.load_program_config", lambda _p: _Program()
    )
    monkeypatch.setattr(
        "greenfloor.cli.offers_lifecycle.cloud_wallet_adapter.new_cloud_wallet_adapter",
        lambda _p: type(
            "_Wallet",
            (),
            {"vault_id": "wallet-1", "get_wallet": staticmethod(lambda: {"offers": []})},
        )(),
    )

    try:
        offers_cancel(
            program_path=program,
            offer_ids=["Offer_pending"],
            cancel_open=False,
            markets_path=markets,
            submit_onchain_after_offchain=True,
            onchain_market_id=None,
            onchain_pair=None,
        )
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert str(exc) == "provide exactly one of --market-id or --pair"
