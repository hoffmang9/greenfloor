from __future__ import annotations

import greenfloor.signing as signing_mod


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
