from __future__ import annotations

import greenfloor.signing as signing_mod


def test_agg_sig_additional_data_matches_chia_network_constants() -> None:
    assert signing_mod._AGG_SIG_ADDITIONAL_DATA_BY_NETWORK["mainnet"] == bytes.fromhex(
        "ccd5bb71183532bff220ba46c268991a3ff07eb358e8255a65c30a2dce0e5fbb"
    )
    assert signing_mod._AGG_SIG_ADDITIONAL_DATA_BY_NETWORK["testnet11"] == bytes.fromhex(
        "37a90eb5185a9c4439a91ddc98bbadce7b4feba060d50116a067de66bf236615"
    )


def test_build_signed_spend_bundle_missing_key_id() -> None:
    result = signing_mod.build_signed_spend_bundle(
        {"network": "mainnet", "receive_address": "xch1abc", "keyring_yaml_path": "/tmp/k.yaml"}
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "missing_key_or_network_or_address"


def test_build_signed_spend_bundle_missing_network() -> None:
    result = signing_mod.build_signed_spend_bundle(
        {"key_id": "k1", "receive_address": "xch1abc", "keyring_yaml_path": "/tmp/k.yaml"}
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "missing_key_or_network_or_address"


def test_build_signed_spend_bundle_missing_address() -> None:
    result = signing_mod.build_signed_spend_bundle(
        {"key_id": "k1", "network": "mainnet", "keyring_yaml_path": "/tmp/k.yaml"}
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "missing_key_or_network_or_address"


def test_build_signed_spend_bundle_missing_keyring() -> None:
    result = signing_mod.build_signed_spend_bundle(
        {"key_id": "k1", "network": "mainnet", "receive_address": "xch1abc"}
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "missing_keyring_yaml_path"


def test_build_signed_spend_bundle_unsupported_asset() -> None:
    result = signing_mod.build_signed_spend_bundle(
        {
            "key_id": "k1",
            "network": "mainnet",
            "receive_address": "xch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "asset_id": "cat_unsupported",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 1},
        }
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "asset_not_supported_yet"


def test_build_signed_spend_bundle_invalid_plan() -> None:
    result = signing_mod.build_signed_spend_bundle(
        {
            "key_id": "k1",
            "network": "mainnet",
            "receive_address": "xch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "asset_id": "xch",
            "plan": {"op_type": "invalid"},
        }
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "invalid_plan"


def test_build_signed_spend_bundle_sdk_import_error(monkeypatch) -> None:
    def _fail_import():
        raise ImportError("no chia_wallet_sdk")

    monkeypatch.setattr(signing_mod, "_import_sdk", _fail_import)
    result = signing_mod.build_signed_spend_bundle(
        {
            "key_id": "k1",
            "network": "mainnet",
            "receive_address": "xch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "asset_id": "xch",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 1},
        }
    )
    assert result["status"] == "skipped"
    assert "wallet_sdk_import_error" in result["reason"]


def test_build_signed_spend_bundle_no_coins(monkeypatch) -> None:
    monkeypatch.setattr(signing_mod, "_import_sdk", lambda: object())
    monkeypatch.setattr(signing_mod, "_list_unspent_xch_coins", lambda **_kw: [])
    result = signing_mod.build_signed_spend_bundle(
        {
            "key_id": "k1",
            "network": "mainnet",
            "receive_address": "xch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "asset_id": "xch",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 1},
        }
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "no_unspent_xch_coins"


def test_build_signed_spend_bundle_offer_missing_request_asset_id() -> None:
    monkeypatch_result = signing_mod.build_signed_spend_bundle(
        {
            "key_id": "k1",
            "network": "mainnet",
            "receive_address": "xch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "asset_id": "xch",
            "plan": {
                "op_type": "offer",
                "offer_asset_id": "xch",
                "offer_amount": 10,
                "request_amount": 1,
            },
        }
    )
    assert monkeypatch_result["status"] == "skipped"
    assert monkeypatch_result["reason"] == "missing_request_asset_id"


def test_build_signed_spend_bundle_offer_delegates_to_offer_builder(monkeypatch) -> None:
    monkeypatch.setattr(signing_mod, "_import_sdk", lambda: object())
    monkeypatch.setattr(
        signing_mod,
        "_build_offer_spend_bundle",
        lambda **_kw: ("aabb", None),
    )
    result = signing_mod.build_signed_spend_bundle(
        {
            "key_id": "k1",
            "network": "testnet11",
            "receive_address": "xch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "asset_id": "xch",
            "plan": {
                "op_type": "offer",
                "offer_asset_id": "xch",
                "offer_amount": 10,
                "request_asset_id": "xch",
                "request_amount": 2,
            },
        }
    )
    assert result["status"] == "executed"
    assert result["spend_bundle_hex"] == "aabb"


def test_sign_and_broadcast_propagates_signing_failure(monkeypatch) -> None:
    monkeypatch.setattr(
        signing_mod,
        "build_signed_spend_bundle",
        lambda _p: {"status": "skipped", "reason": "no_unspent_xch_coins"},
    )
    result = signing_mod.sign_and_broadcast(
        {
            "key_id": "k1",
            "network": "mainnet",
            "receive_address": "xch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 1},
        }
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "no_unspent_xch_coins"
    assert result["operation_id"] is None


def test_sign_and_broadcast_calls_broadcast(monkeypatch) -> None:
    broadcast_called = {}

    def _fake_broadcast(*, sdk, spend_bundle_hex, network):
        broadcast_called["hex"] = spend_bundle_hex
        broadcast_called["network"] = network
        return {"status": "executed", "reason": "submitted", "operation_id": "tx-abc"}

    monkeypatch.setattr(
        signing_mod,
        "build_signed_spend_bundle",
        lambda _p: {
            "status": "executed",
            "reason": "signing_success",
            "spend_bundle_hex": "aabb",
        },
    )

    class _FakeSdk:
        pass

    monkeypatch.setattr(signing_mod, "_import_sdk", lambda: _FakeSdk)
    monkeypatch.setattr(signing_mod, "_broadcast_spend_bundle", _fake_broadcast)

    result = signing_mod.sign_and_broadcast(
        {
            "key_id": "k1",
            "network": "testnet11",
            "receive_address": "xch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 1},
        }
    )
    assert result["status"] == "executed"
    assert result["operation_id"] == "tx-abc"
    assert broadcast_called["hex"] == "aabb"
    assert broadcast_called["network"] == "testnet11"


def test_build_additions_from_plan_split() -> None:
    additions, error = signing_mod._build_additions_from_plan(
        plan={"op_type": "split", "size_base_units": 10, "op_count": 2},
        selected_coins=[{"amount": 25}],
        receive_address="xch1addr",
    )
    assert error is None
    assert additions is not None
    assert len(additions) == 3
    assert additions[0] == {"address": "xch1addr", "amount": 10}
    assert additions[1] == {"address": "xch1addr", "amount": 10}
    assert additions[2] == {"address": "xch1addr", "amount": 5}


def test_build_additions_from_plan_insufficient() -> None:
    additions, error = signing_mod._build_additions_from_plan(
        plan={"op_type": "split", "size_base_units": 100, "op_count": 2},
        selected_coins=[{"amount": 10}],
        receive_address="xch1addr",
    )
    assert additions is None
    assert error == "insufficient_selected_coin_total"


def test_parse_fingerprint_direct_integer() -> None:
    assert signing_mod._parse_fingerprint("123456") == 123456


def test_parse_fingerprint_prefix() -> None:
    assert signing_mod._parse_fingerprint("fingerprint:789") == 789


def test_parse_fingerprint_unknown_returns_none() -> None:
    assert signing_mod._parse_fingerprint("unknown_key") is None


def test_signing_uses_testnet11_coinset_adapter_network(monkeypatch) -> None:
    captured = {}

    class _FakeAdapter:
        def __init__(self, base_url=None, *, network="mainnet", require_testnet11=False) -> None:
            captured["base_url"] = base_url
            captured["network"] = network
            captured["require_testnet11"] = require_testnet11

        def get_coin_records_by_puzzle_hash(
            self, *, puzzle_hash_hex: str, include_spent_coins: bool
        ):
            _ = puzzle_hash_hex
            _ = include_spent_coins
            return []

    class _FakeAddressObj:
        def __init__(self) -> None:
            self.puzzle_hash = b"\x11" * 32

    class _FakeAddress:
        @staticmethod
        def decode(value: str):
            _ = value
            return _FakeAddressObj()

    class _FakeSdk:
        Address = _FakeAddress

    monkeypatch.setattr(signing_mod, "_import_sdk", lambda: _FakeSdk)
    monkeypatch.setattr(signing_mod, "CoinsetAdapter", _FakeAdapter)
    monkeypatch.delenv("GREENFLOOR_COINSET_BASE_URL", raising=False)

    result = signing_mod.build_signed_spend_bundle(
        {
            "key_id": "k1",
            "network": "testnet11",
            "receive_address": "xch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "asset_id": "xch",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 1},
        }
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "no_unspent_xch_coins"
    assert captured["network"] == "testnet11"
    assert captured["base_url"] is None
    assert captured["require_testnet11"] is True


def test_from_input_spend_bundle_xch_prefers_new_binding() -> None:
    calls = {}

    class _Sdk:
        @staticmethod
        def from_input_spend_bundle_xch(bundle, requested):
            calls["new"] = (bundle, requested)
            return "new-path"

    result = signing_mod._from_input_spend_bundle_xch(
        sdk=_Sdk,
        input_spend_bundle="bundle",
        requested_payments_xch=["np"],
    )
    assert result == "new-path"
    assert calls["new"] == ("bundle", ["np"])


def test_from_input_spend_bundle_xch_falls_back_to_legacy_binding() -> None:
    calls = {}

    class _Sdk:
        @staticmethod
        def from_input_spend_bundle(bundle, requested):
            calls["legacy"] = (bundle, requested)
            return "legacy-path"

    result = signing_mod._from_input_spend_bundle_xch(
        sdk=_Sdk,
        input_spend_bundle="bundle",
        requested_payments_xch=["np"],
    )
    assert result == "legacy-path"
    assert calls["legacy"] == ("bundle", ["np"])


def test_from_input_spend_bundle_xch_requires_supported_binding() -> None:
    class _Sdk:
        pass

    try:
        signing_mod._from_input_spend_bundle_xch(
            sdk=_Sdk,
            input_spend_bundle="bundle",
            requested_payments_xch=["np"],
        )
        raise AssertionError("expected RuntimeError")
    except RuntimeError as exc:
        assert str(exc) == "wallet_sdk_from_input_spend_bundle_xch_unavailable"
