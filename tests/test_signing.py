from __future__ import annotations

import hashlib
from typing import Any

import greenfloor.adapters.bls_signing as signing_mod
import greenfloor.adapters.native_offer as native_offer_mod
import greenfloor.signing_clvm as signing_clvm_mod
from tests.support import bls_signing_broadcast as broadcast_support

_AGG_SIG_ADDITIONAL_DATA_BY_NETWORK = {
    "mainnet": bytes.fromhex("ccd5bb71183532bff220ba46c268991a3ff07eb358e8255a65c30a2dce0e5fbb"),
    "testnet11": bytes.fromhex("37a90eb5185a9c4439a91ddc98bbadce7b4feba060d50116a067de66bf236615"),
}


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
    targets = signing_clvm_mod._extract_required_bls_targets_for_conditions(
        conditions=[_Condition()],
        coin=_Coin(),
        agg_sig_me_additional_data=additional_data,
    )
    assert len(targets) == 1
    pk, message = targets[0]
    assert pk == b"\x11" * 48
    assert message == b"\xaa\xbb" + (b"\x03" * 32) + additional_data


def test_agg_sig_additional_data_matches_chia_network_constants() -> None:
    assert _AGG_SIG_ADDITIONAL_DATA_BY_NETWORK["mainnet"] == bytes.fromhex(
        "ccd5bb71183532bff220ba46c268991a3ff07eb358e8255a65c30a2dce0e5fbb"
    )
    assert _AGG_SIG_ADDITIONAL_DATA_BY_NETWORK["testnet11"] == bytes.fromhex(
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


def test_canonical_is_xch_requires_explicit_symbol() -> None:
    from greenfloor.hex_utils import canonical_is_xch

    assert canonical_is_xch("xch")
    assert canonical_is_xch("TXCH")
    assert not canonical_is_xch("")
    assert not canonical_is_xch("a" * 64)


def test_build_signed_spend_bundle_empty_asset_id_not_treated_as_xch() -> None:
    result = signing_mod.build_signed_spend_bundle(
        {
            "key_id": "k1",
            "network": "mainnet",
            "receive_address": "xch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "asset_id": "",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 1},
        }
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "asset_not_supported_yet"


def test_build_signed_spend_bundle_invalid_plan(monkeypatch) -> None:
    monkeypatch.setattr(
        signing_mod,
        "_load_master_private_key",
        lambda *_args, **_kwargs: (b"\x01" * 32, None),
    )
    monkeypatch.setattr(
        signing_mod,
        "_call_signer_build",
        lambda *_args, **_kwargs: (None, "unsupported_operation_type"),
    )
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
    assert result["reason"] == "signing_failed:unsupported_operation_type"


def test_build_signed_spend_bundle_signer_import_error(monkeypatch) -> None:
    def _fail_import():
        raise ImportError("no greenfloor_signer")

    monkeypatch.setattr(
        signing_mod,
        "_load_master_private_key",
        lambda *_args, **_kwargs: (b"\x01" * 32, None),
    )
    monkeypatch.setattr(signing_mod, "import_kernel", _fail_import)
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
    assert result["reason"] == "signing_failed:greenfloor_signer_import_error:no greenfloor_signer"


def test_build_signed_spend_bundle_no_coins(monkeypatch) -> None:
    monkeypatch.setattr(
        signing_mod,
        "_load_master_private_key",
        lambda *_args, **_kwargs: (b"\x01" * 32, None),
    )
    monkeypatch.setattr(
        signing_mod,
        "_call_signer_build",
        lambda *_args, **_kwargs: (None, "no_unspent_xch_coins"),
    )
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
    assert result["reason"] == "signing_failed:no_unspent_xch_coins"


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
    captured: dict[str, Any] = {}

    monkeypatch.setattr(
        signing_mod,
        "_load_master_private_key",
        lambda *_args, **_kwargs: (b"\x01" * 32, None),
    )

    def _fake_call(method_name: str, network: str, master_sk_bytes: bytes, request: dict) -> tuple:
        _ = network, master_sk_bytes
        captured["method"] = method_name
        captured["request"] = request
        return ("aabb", None)

    monkeypatch.setattr(signing_mod, "_call_signer_build", _fake_call)
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
    assert captured["request"].get("offer_coin_ids") == ["abcdef"]


def test_build_signed_spend_bundle_offer_propagates_missing_agg_sig_targets(monkeypatch) -> None:
    monkeypatch.setattr(
        signing_mod,
        "_load_master_private_key",
        lambda *_args, **_kwargs: (b"\x01" * 32, None),
    )
    monkeypatch.setattr(
        signing_mod,
        "_call_signer_build",
        lambda *_args, **_kwargs: (None, "no_agg_sig_targets_found"),
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

    def _fake_broadcast(*, spend_bundle_hex, network):
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

    monkeypatch.setattr(signing_mod, "_broadcast_bls_spend_bundle_rust", _fake_broadcast)

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


def test_sign_and_broadcast_mixed_split_propagates_signing_failure(monkeypatch) -> None:
    monkeypatch.setattr(
        signing_mod,
        "_build_mixed_split_spend_bundle",
        lambda _payload: (None, "missing_output_amounts"),
    )
    result = signing_mod.sign_and_broadcast_mixed_split(
        {
            "key_id": "k1",
            "network": "mainnet",
            "receive_address": "xch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "asset_id": "xch",
            "output_amounts_base_units": [1],
        }
    )
    assert result["status"] == "skipped"
    assert result["reason"] == "signing_failed:missing_output_amounts"
    assert result["operation_id"] is None


def test_sign_and_broadcast_mixed_split_calls_broadcast(monkeypatch) -> None:
    broadcast_called = {}

    monkeypatch.setattr(
        signing_mod, "_build_mixed_split_spend_bundle", lambda _payload: ("aabb", None)
    )

    def _fake_broadcast(*, spend_bundle_hex, network):
        broadcast_called["hex"] = spend_bundle_hex
        broadcast_called["network"] = network
        return {"status": "executed", "reason": "submitted", "operation_id": "tx-mixed"}

    monkeypatch.setattr(signing_mod, "_broadcast_bls_spend_bundle_rust", _fake_broadcast)
    result = signing_mod.sign_and_broadcast_mixed_split(
        {
            "key_id": "k1",
            "network": "testnet11",
            "receive_address": "txch1abc",
            "keyring_yaml_path": "/tmp/k.yaml",
            "asset_id": "xch",
            "output_amounts_base_units": [1, 10, 100],
        }
    )
    assert result["status"] == "executed"
    assert result["operation_id"] == "tx-mixed"
    assert broadcast_called["hex"] == "aabb"
    assert broadcast_called["network"] == "testnet11"


def test_coin_id_set_accepts_hex_with_or_without_prefix() -> None:
    ids = signing_mod._coin_id_set(
        [
            "0x" + ("ab" * 32),
            "ab" * 32,
            "0x" + ("cd" * 32),
            "not-hex",
        ]
    )
    assert ids == {("ab" * 32), ("cd" * 32)}


def test_parse_fingerprint_direct_integer() -> None:
    from tests.support.bls_signing_keys import parse_fingerprint

    assert parse_fingerprint("123456") == 123456


def test_parse_fingerprint_prefix() -> None:
    from tests.support.bls_signing_keys import parse_fingerprint

    assert parse_fingerprint("fingerprint:789") == 789


def test_parse_fingerprint_unknown_returns_none() -> None:
    from tests.support.bls_signing_keys import parse_fingerprint

    assert parse_fingerprint("unknown_key") is None


def test_signing_split_path_passes_testnet11_network_to_rust(monkeypatch) -> None:
    captured: dict[str, str] = {}

    monkeypatch.setattr(
        signing_mod,
        "_load_master_private_key",
        lambda *_args, **_kwargs: (b"\x01" * 32, None),
    )

    def _fake_call(_method: str, network: str, _sk: bytes, _request: dict) -> tuple:
        captured["network"] = network
        return None, "no_unspent_xch_coins"

    monkeypatch.setattr(signing_mod, "_call_signer_build", _fake_call)

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
    assert result["reason"] == "signing_failed:no_unspent_xch_coins"
    assert captured["network"] == "testnet11"


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

    monkeypatch.setattr(native_offer_mod, "import_kernel", lambda: _Native)

    result = native_offer_mod.from_input_spend_bundle_xch(
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

    monkeypatch.setattr(native_offer_mod, "import_kernel", lambda: _Native)

    result = native_offer_mod.from_input_spend_bundle_xch(
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

    monkeypatch.setattr(native_offer_mod, "import_kernel", lambda: _Native)

    try:
        native_offer_mod.from_input_spend_bundle_xch(
            sdk=_Sdk,
            input_spend_bundle=_InputSpendBundle(),
            requested_payments_xch=[_NotarizedPayment()],
        )
        raise AssertionError("expected RuntimeError")
    except RuntimeError as exc:
        assert str(exc) == "native_failure"


def test_domain_bytes_for_agg_sig_kind_variants() -> None:
    additional = bytes.fromhex("37a90eb5185a9c4439a91ddc98bbadce7b4feba060d50116a067de66bf236615")
    assert signing_clvm_mod._domain_bytes_for_agg_sig_kind("unsafe", additional) is None
    assert signing_clvm_mod._domain_bytes_for_agg_sig_kind("me", additional) == additional
    expected_parent = hashlib.sha256(additional + bytes([43])).digest()
    assert signing_clvm_mod._domain_bytes_for_agg_sig_kind("parent", additional) == expected_parent
    assert signing_clvm_mod._domain_bytes_for_agg_sig_kind("unknown_kind", additional) is None


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

    targets = signing_clvm_mod._extract_required_bls_targets_for_conditions(
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

    result = broadcast_support._broadcast_spend_bundle(
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

    result = broadcast_support._broadcast_spend_bundle(
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

    monkeypatch.setattr(broadcast_support, "_coinset_adapter", lambda *, network: _FailingAdapter())
    result = broadcast_support._broadcast_spend_bundle(
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

    monkeypatch.setattr(broadcast_support, "_coinset_adapter", lambda *, network: _Adapter())
    result = broadcast_support._broadcast_spend_bundle(
        sdk=_Sdk,
        spend_bundle_hex="aabb",
        network="mainnet",
    )
    assert captured["hex"] == "aabb"
    assert result["status"] == "executed"
    assert result["reason"] == "submitted"
    assert result["operation_id"] == ("99" * 32)


def test_broadcast_spend_bundle_falls_back_to_structured_payload(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _Coin:
        parent_coin_info = bytes.fromhex("11" * 32)
        puzzle_hash = bytes.fromhex("22" * 32)
        amount = 7

    class _CoinSpend:
        coin = _Coin()
        puzzle_reveal = bytes.fromhex("ff")
        solution = bytes.fromhex("80")

    class _Signature:
        @staticmethod
        def to_bytes() -> bytes:
            return bytes.fromhex("aa" * 96)

    class _SpendBundleObj:
        coin_spends = [_CoinSpend()]
        aggregated_signature = _Signature()

        @staticmethod
        def hash() -> bytes:
            return b"\x77" * 32

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
            return {
                "success": False,
                "error": 'invalid type: string "abcd", expected struct SpendBundle',
            }

        def push_tx_structured(self, *, spend_bundle: dict[str, Any]):
            captured["structured"] = spend_bundle
            return {"success": True, "status": "submitted"}

    monkeypatch.setattr(broadcast_support, "_coinset_adapter", lambda *, network: _Adapter())
    result = broadcast_support._broadcast_spend_bundle(
        sdk=_Sdk,
        spend_bundle_hex="aabb",
        network="mainnet",
    )
    assert captured["hex"] == "aabb"
    structured = captured["structured"]
    assert isinstance(structured, dict)
    assert structured["aggregated_signature"].startswith("0x")
    assert structured["coin_spends"][0]["coin"]["amount"] == 7
    assert result["status"] == "executed"
    assert result["reason"] == "submitted"
    assert result["operation_id"] == ("77" * 32)


def test_build_mixed_split_rejects_sub_unit_cat_outputs(monkeypatch) -> None:
    monkeypatch.setattr(
        signing_mod,
        "_load_master_private_key",
        lambda *_args, **_kwargs: (b"\x01" * 32, None),
    )

    class _Signer:
        @staticmethod
        def build_bls_mixed_split(_network: str, _sk: bytes, _request: dict) -> dict:
            return {"error": "cat_output_below_minimum_mojos"}

    monkeypatch.setattr(signing_mod, "import_kernel", lambda: _Signer())

    spend_bundle_hex, err = signing_mod._build_mixed_split_spend_bundle(
        {
            "key_id": "key-1",
            "network": "mainnet",
            "receive_address": "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w",
            "keyring_yaml_path": "/tmp/keyring.yaml",
            "asset_id": "a" * 64,
            "selected_coin_ids": ["b" * 64, "c" * 64],
            "output_amounts_base_units": [999],
            "fee_mojos": 0,
        }
    )
    assert spend_bundle_hex is None
    assert err == "cat_output_below_minimum_mojos"


def test_build_mixed_split_allow_sub_cat_output_bypasses_floor_guard(monkeypatch) -> None:
    monkeypatch.setattr(
        signing_mod,
        "_load_master_private_key",
        lambda *_args, **_kwargs: (b"\x01" * 32, None),
    )
    monkeypatch.setattr(
        signing_mod,
        "_call_signer_build",
        lambda *_args, **_kwargs: (None, "sentinel_requested_coin_resolution_error"),
    )

    spend_bundle_hex, err = signing_mod._build_mixed_split_spend_bundle(
        {
            "key_id": "key-1",
            "network": "mainnet",
            "receive_address": "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w",
            "keyring_yaml_path": "/tmp/keyring.yaml",
            "asset_id": "a" * 64,
            "selected_coin_ids": ["b" * 64, "c" * 64],
            "output_amounts_base_units": [999],
            "fee_mojos": 0,
            "allow_sub_cat_output": True,
        }
    )
    # The override should bypass the minimum-output guard; deeper validation
    # can still fail based on runtime environment and available wallet context.
    assert spend_bundle_hex is None
    assert err == "sentinel_requested_coin_resolution_error"
