from __future__ import annotations

import hashlib

import greenfloor.signing as signing_mod


def test_extract_required_bls_targets_for_conditions_agg_sig_me() -> None:
    class _Pk:
        def to_bytes(self) -> bytes:
            return b"\x11" * 48

    class _Parsed:
        public_key = _Pk()
        message = b"\xaa\xbb"

    class _Condition:
        @staticmethod
        def parse_agg_sig_me():
            return _Parsed()

    class _Coin:
        parent_coin_info = b"\x01" * 32
        puzzle_hash = b"\x02" * 32
        amount = 7

        @staticmethod
        def coin_id() -> bytes:
            return b"\x03" * 32

    additional_data = bytes.fromhex(
        "37a90eb5185a9c4439a91ddc98bbadce7b4feba060d50116a067de66bf236615"
    )
    targets = signing_mod._extract_required_bls_targets_for_conditions(
        conditions=[_Condition()],
        coin=_Coin(),
        agg_sig_me_additional_data=additional_data,
    )
    assert len(targets) == 1
    pk, message = targets[0]
    assert pk == b"\x11" * 48
    assert message == b"\xaa\xbb" + (b"\x03" * 32) + additional_data


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
    captured: dict[str, object] = {}

    monkeypatch.setattr(signing_mod, "_import_sdk", lambda: object())

    def _fake_build_offer_spend_bundle(**kwargs):
        captured.update(kwargs)
        return ("aabb", None)

    monkeypatch.setattr(signing_mod, "_build_offer_spend_bundle", _fake_build_offer_spend_bundle)
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
                "offer_coin_ids": ["ABCDEF"],
            },
        }
    )
    assert result["status"] == "executed"
    assert result["spend_bundle_hex"] == "aabb"
    assert captured.get("offer_coin_ids") == ["abcdef"]


def test_build_signed_spend_bundle_offer_propagates_missing_agg_sig_targets(monkeypatch) -> None:
    monkeypatch.setattr(signing_mod, "_import_sdk", lambda: object())
    monkeypatch.setattr(
        signing_mod,
        "_build_offer_spend_bundle",
        lambda **_kw: (None, "no_agg_sig_targets_found"),
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
    assert result["status"] == "skipped"
    assert result["reason"] == "signing_failed:no_agg_sig_targets_found"


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


def test_from_input_spend_bundle_xch_calls_greenfloor_native(monkeypatch) -> None:
    calls = {}

    class _InputSpendBundle:
        @staticmethod
        def to_bytes() -> bytes:
            return b"input-bytes"

    class _Native:
        @staticmethod
        def from_input_spend_bundle_xch(spend_bundle_bytes, requested):
            calls["native"] = (spend_bundle_bytes, requested)
            return b"result-bytes"

    class _SpendBundleType:
        @staticmethod
        def from_bytes(value: bytes):
            calls["from_bytes"] = value
            return "rebuilt-spend-bundle"

    class _Sdk:
        SpendBundle = _SpendBundleType

    class _Payment:
        puzzle_hash = b"\x11" * 32
        amount = 42

    class _NotarizedPayment:
        nonce = b"\x22" * 32
        payments = [_Payment()]

    monkeypatch.setattr(signing_mod, "_import_greenfloor_native", lambda: _Native)

    result = signing_mod._from_input_spend_bundle_xch(
        sdk=_Sdk,
        input_spend_bundle=_InputSpendBundle(),
        requested_payments_xch=[_NotarizedPayment()],
    )
    assert result == "rebuilt-spend-bundle"
    assert calls["native"] == (b"input-bytes", [(b"\x22" * 32, [(b"\x11" * 32, 42)])])
    assert calls["from_bytes"] == b"result-bytes"


def test_from_input_spend_bundle_xch_supports_sdk_byte_wrapper_types(monkeypatch) -> None:
    class _ByteWrapper:
        def __init__(self, value: bytes) -> None:
            self._value = value

        def to_bytes(self) -> bytes:
            return self._value

    class _InputSpendBundle:
        @staticmethod
        def to_bytes() -> bytes:
            return b"input-bytes"

    class _Native:
        @staticmethod
        def from_input_spend_bundle_xch(_spend_bundle_bytes, requested):
            assert requested == [(b"\xaa" * 32, [(b"\xbb" * 32, 9)])]
            return b"result-bytes"

    class _SpendBundleType:
        @staticmethod
        def from_bytes(value: bytes):
            assert value == b"result-bytes"
            return "rebuilt-spend-bundle"

    class _Sdk:
        SpendBundle = _SpendBundleType

    class _Payment:
        puzzle_hash = _ByteWrapper(b"\xbb" * 32)
        amount = 9

    class _NotarizedPayment:
        nonce = _ByteWrapper(b"\xaa" * 32)
        payments = [_Payment()]

    monkeypatch.setattr(signing_mod, "_import_greenfloor_native", lambda: _Native)

    result = signing_mod._from_input_spend_bundle_xch(
        sdk=_Sdk,
        input_spend_bundle=_InputSpendBundle(),
        requested_payments_xch=[_NotarizedPayment()],
    )
    assert result == "rebuilt-spend-bundle"


def test_from_input_spend_bundle_xch_propagates_native_errors(monkeypatch) -> None:
    class _InputSpendBundle:
        @staticmethod
        def to_bytes() -> bytes:
            return b"input-bytes"

    class _Native:
        @staticmethod
        def from_input_spend_bundle_xch(_spend_bundle_bytes, _requested):
            raise RuntimeError("native_failure")

    class _Sdk:
        class SpendBundle:
            @staticmethod
            def from_bytes(_value):
                raise AssertionError("should not be called")

    class _Payment:
        puzzle_hash = b"\x11" * 32
        amount = 42

    class _NotarizedPayment:
        nonce = b"\x22" * 32
        payments = [_Payment()]

    monkeypatch.setattr(signing_mod, "_import_greenfloor_native", lambda: _Native)

    try:
        signing_mod._from_input_spend_bundle_xch(
            sdk=_Sdk,
            input_spend_bundle=_InputSpendBundle(),
            requested_payments_xch=[_NotarizedPayment()],
        )
        raise AssertionError("expected RuntimeError")
    except RuntimeError as exc:
        assert str(exc) == "native_failure"


def test_domain_bytes_for_agg_sig_kind_variants() -> None:
    additional = bytes.fromhex("37a90eb5185a9c4439a91ddc98bbadce7b4feba060d50116a067de66bf236615")
    assert signing_mod._domain_bytes_for_agg_sig_kind("unsafe", additional) is None
    assert signing_mod._domain_bytes_for_agg_sig_kind("me", additional) == additional
    expected_parent = hashlib.sha256(additional + bytes([43])).digest()
    assert signing_mod._domain_bytes_for_agg_sig_kind("parent", additional) == expected_parent
    assert signing_mod._domain_bytes_for_agg_sig_kind("unknown_kind", additional) is None


def test_extract_required_bls_targets_for_conditions_agg_sig_unsafe() -> None:
    class _Pk:
        def to_bytes(self) -> bytes:
            return b"\x12" * 48

    class _Parsed:
        public_key = _Pk()
        message = b"\xfe\xed"

    class _Condition:
        @staticmethod
        def parse_agg_sig_unsafe():
            return _Parsed()

    class _Coin:
        parent_coin_info = b"\x01" * 32
        puzzle_hash = b"\x02" * 32
        amount = 7

        @staticmethod
        def coin_id() -> bytes:
            return b"\x03" * 32

    targets = signing_mod._extract_required_bls_targets_for_conditions(
        conditions=[_Condition()],
        coin=_Coin(),
        agg_sig_me_additional_data=b"\xaa" * 32,
    )
    assert len(targets) == 1
    pk, message = targets[0]
    assert pk == b"\x12" * 48
    # unsafe kind does not append coin info or domain bytes
    assert message == b"\xfe\xed"


def test_broadcast_spend_bundle_invalid_hex_returns_skipped() -> None:
    class _Sdk:
        class SpendBundle:
            @staticmethod
            def from_bytes(_value):
                raise AssertionError("from_bytes should not be called")

    result = signing_mod._broadcast_spend_bundle(
        sdk=_Sdk,
        spend_bundle_hex="zz-not-hex",
        network="mainnet",
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "invalid_spend_bundle_hex"
    assert result["operation_id"] is None


def test_broadcast_spend_bundle_decode_error_returns_skipped() -> None:
    class _Sdk:
        class SpendBundle:
            @staticmethod
            def from_bytes(_value):
                raise RuntimeError("decode_failed")

    result = signing_mod._broadcast_spend_bundle(
        sdk=_Sdk,
        spend_bundle_hex="aabb",
        network="mainnet",
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "spend_bundle_decode_error:decode_failed"
    assert result["operation_id"] is None


def test_broadcast_spend_bundle_push_tx_error_returns_skipped(monkeypatch) -> None:
    class _SpendBundleObj:
        @staticmethod
        def hash() -> bytes:
            return b"\x00" * 32

    class _Sdk:
        class SpendBundle:
            @staticmethod
            def from_bytes(_value):
                return _SpendBundleObj()

    class _FailingAdapter:
        def push_tx(self, *, spend_bundle_hex: str):
            _ = spend_bundle_hex
            raise RuntimeError("coinset_down")

    monkeypatch.setattr(signing_mod, "_coinset_adapter", lambda *, network: _FailingAdapter())
    result = signing_mod._broadcast_spend_bundle(
        sdk=_Sdk,
        spend_bundle_hex="aabb",
        network="mainnet",
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "push_tx_error:coinset_down"
    assert result["operation_id"] is None


def test_broadcast_spend_bundle_success_returns_operation_id(monkeypatch) -> None:
    captured: dict[str, str] = {}

    class _SpendBundleObj:
        @staticmethod
        def hash() -> bytes:
            return b"\x99" * 32

    class _Sdk:
        @staticmethod
        def to_hex(value: bytes) -> str:
            return value.hex()

        class SpendBundle:
            @staticmethod
            def from_bytes(_value):
                return _SpendBundleObj()

    class _Adapter:
        def push_tx(self, *, spend_bundle_hex: str):
            captured["hex"] = spend_bundle_hex
            return {"success": True, "status": "submitted"}

    monkeypatch.setattr(signing_mod, "_coinset_adapter", lambda *, network: _Adapter())
    result = signing_mod._broadcast_spend_bundle(
        sdk=_Sdk,
        spend_bundle_hex="aabb",
        network="mainnet",
    )
    assert captured["hex"] == "aabb"
    assert result["status"] == "executed"
    assert result["reason"] == "submitted"
    assert result["operation_id"] == ("99" * 32)
