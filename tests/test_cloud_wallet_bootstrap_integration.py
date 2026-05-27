from __future__ import annotations

from pathlib import Path
from typing import Any, cast

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.runtime.cloud_wallet.bootstrap import ensure_offer_bootstrap_denominations


def test_ensure_offer_bootstrap_denominations_surfaces_wait_error(
    monkeypatch, tmp_path: Path
) -> None:
    keyring_path = tmp_path / "keyring.yaml"
    keyring_path.write_text("keys: []\n", encoding="utf-8")

    class _Program:
        app_network = "mainnet"
        coin_ops_minimum_fee_mojos = 0
        cloud_wallet_base_url = "https://api.vault.chia.net"
        cloud_wallet_user_key_id = "k"
        cloud_wallet_private_key_pem_path = "/tmp/key.pem"
        cloud_wallet_vault_id = "Wallet_abc"
        cloud_wallet_kms_key_id = ""
        cloud_wallet_kms_region = ""
        cloud_wallet_kms_public_key_hex = ""

    class _LadderEntry:
        size_base_units = 1
        target_count = 2
        split_buffer_count = 0

    class _Market:
        ladders = {"sell": [_LadderEntry()]}
        receive_address = "xch1test"
        base_asset = "xch"
        pricing = {}

    class _Wallet:
        @staticmethod
        def list_coins(*, asset_id=None, include_pending=False):
            _ = asset_id, include_pending
            return [{"id": "coin_big", "amount": 10, "state": "CONFIRMED"}]

    class _Plan:
        source_coin_id = "coin_big"
        source_amount = 10
        output_amounts_base_units = [1, 1]
        total_output_amount = 2
        change_amount = 8
        deficits = []

    class _Deficit:
        size_base_units = 1
        deficit_count = 2
        required_count = 2
        current_count = 0

    _plan = _Plan()
    _plan.deficits = [_Deficit()]

    result = ensure_offer_bootstrap_denominations(
        program=cast(ProgramConfig, _Program()),
        market=cast(MarketConfig, _Market()),
        wallet=cast(CloudWalletAdapter, _Wallet()),
        resolved_base_asset_id="xch",
        resolved_quote_asset_id="wusdc",
        quote_price=0.999,
        plan_bootstrap_mixed_outputs_fn=lambda **_k: _plan,
        resolve_bootstrap_split_fee_fn=lambda **_k: (0, "coinset_conservative", None),
        wait_for_mempool_then_confirmation_fn=lambda **_k: (_ for _ in ()).throw(
            RuntimeError("confirmation_wait_timeout")
        ),
        split_coins_fn=lambda **_kw: {"signature_request_id": "sr-1", "status": "SUBMITTED"},
        poll_signature_request_until_not_unsigned_fn=lambda **_kw: ("SUBMITTED", []),
    )
    assert result["status"] == "failed"
    assert result["reason"] == "bootstrap_wait_failed"
    assert result["wait_error"] == "confirmation_wait_timeout"
    assert result["fallback_to_cloud_wallet_offer_split"] is True


def test_ensure_offer_bootstrap_denominations_reports_fee_balance_guidance(
    monkeypatch, tmp_path: Path
) -> None:
    keyring_path = tmp_path / "keyring.yaml"
    keyring_path.write_text("keys: []\n", encoding="utf-8")

    class _Program:
        app_network = "mainnet"
        coin_ops_minimum_fee_mojos = 0
        cloud_wallet_base_url = "https://api.vault.chia.net"
        cloud_wallet_user_key_id = "k"
        cloud_wallet_private_key_pem_path = "/tmp/key.pem"
        cloud_wallet_vault_id = "Wallet_abc"
        cloud_wallet_kms_key_id = ""
        cloud_wallet_kms_region = ""
        cloud_wallet_kms_public_key_hex = ""

    class _LadderEntry:
        size_base_units = 1
        target_count = 2
        split_buffer_count = 0

    class _Market:
        ladders = {"sell": [_LadderEntry()]}
        receive_address = "xch1test"
        base_asset = "xch"
        pricing = {}

    class _Wallet:
        @staticmethod
        def list_coins(*, asset_id=None, include_pending=False):
            _ = asset_id, include_pending
            return [{"id": "coin_big", "amount": 10, "state": "CONFIRMED"}]

    class _Plan:
        source_coin_id = "coin_big"
        source_amount = 10
        output_amounts_base_units = [1, 1]
        total_output_amount = 2
        change_amount = 8
        deficits = []

    class _Deficit:
        size_base_units = 1
        deficit_count = 2
        required_count = 2
        current_count = 0

    _plan = _Plan()
    _plan.deficits = [_Deficit()]

    def _failing_split(**_kw: Any) -> dict:
        raise RuntimeError("insufficient_xch_fee_balance_for_mixed_split:required=100:available=0")

    result = ensure_offer_bootstrap_denominations(
        program=cast(ProgramConfig, _Program()),
        market=cast(MarketConfig, _Market()),
        wallet=cast(CloudWalletAdapter, _Wallet()),
        resolved_base_asset_id="xch",
        resolved_quote_asset_id="wusdc",
        quote_price=0.999,
        plan_bootstrap_mixed_outputs_fn=lambda **_k: _plan,
        resolve_bootstrap_split_fee_fn=lambda **_k: (100, "coinset_conservative", None),
        split_coins_fn=_failing_split,
    )
    assert result["status"] == "failed"
    assert "insufficient_xch_fee_balance_for_mixed_split" in str(
        result.get("reason", "") or result.get("error", "")
    )


def test_ensure_offer_bootstrap_denominations_buy_waits_on_quote_asset(
    monkeypatch, tmp_path: Path
) -> None:
    keyring_path = tmp_path / "keyring.yaml"
    keyring_path.write_text("keys: []\n", encoding="utf-8")
    wait_asset_ids: list[str] = []
    list_asset_ids: list[str | None] = []

    class _Program:
        app_network = "mainnet"
        coin_ops_minimum_fee_mojos = 0
        cloud_wallet_base_url = "https://api.vault.chia.net"
        cloud_wallet_user_key_id = "k"
        cloud_wallet_private_key_pem_path = "/tmp/key.pem"
        cloud_wallet_vault_id = "Wallet_abc"
        cloud_wallet_kms_key_id = ""
        cloud_wallet_kms_region = ""
        cloud_wallet_kms_public_key_hex = ""

    class _LadderEntry:
        size_base_units = 10
        target_count = 1
        split_buffer_count = 0

    class _Market:
        ladders = {"buy": [_LadderEntry()]}
        receive_address = "xch1test"
        base_asset = "base_asset"
        quote_asset = "quote_asset"
        pricing = {"quote_unit_mojo_multiplier": 1000}

    class _Wallet:
        @staticmethod
        def list_coins(*, asset_id=None, include_pending=False):
            _ = include_pending
            list_asset_ids.append(asset_id)
            return [{"id": "coin_big", "amount": 50_000, "state": "CONFIRMED"}]

    class _Deficit:
        size_base_units = 10_000
        deficit_count = 1
        required_count = 1
        current_count = 0

    class _Plan:
        source_coin_id = "coin_big"
        source_amount = 50_000
        output_amounts_base_units = [10_000]
        total_output_amount = 10_000
        change_amount = 40_000
        deficits = [_Deficit()]

    result = ensure_offer_bootstrap_denominations(
        program=cast(ProgramConfig, _Program()),
        market=cast(MarketConfig, _Market()),
        wallet=cast(CloudWalletAdapter, _Wallet()),
        resolved_base_asset_id="Asset_base",
        resolved_quote_asset_id="Asset_quote",
        quote_price=1.0,
        action_side="buy",
        plan_bootstrap_mixed_outputs_fn=lambda **_k: _Plan(),
        resolve_bootstrap_split_fee_fn=lambda **_k: (0, "coinset_conservative", None),
        wait_for_mempool_then_confirmation_fn=lambda **kwargs: (
            wait_asset_ids.append(str(kwargs.get("asset_id"))) or []
        ),
        split_coins_fn=lambda **_kw: {"signature_request_id": "sr-1", "status": "SUBMITTED"},
        poll_signature_request_until_not_unsigned_fn=lambda **_kw: ("SUBMITTED", []),
    )
    assert result["status"] == "executed"
    assert wait_asset_ids == ["Asset_quote"]
    assert list_asset_ids[0] == "Asset_quote"
