from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import pytest

from tests.helpers.cloud_wallet_adapter_fixtures import (
    FAKE_KMS_PUBKEY_HEX,
    FakeHttpResponse,
    build_adapter,
    build_kms_adapter,
)


def test_kms_configured_returns_false_without_kms(tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    assert adapter.kms_configured is False


def test_kms_configured_returns_true_with_kms(tmp_path: Path) -> None:
    adapter = build_kms_adapter(tmp_path)
    assert adapter.kms_configured is True


def test_auto_sign_if_kms_is_noop_without_kms(tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    result = {"signature_request_id": "SigReq_1", "status": "UNSIGNED"}
    out = adapter._auto_sign_if_kms(result)
    assert out["status"] == "UNSIGNED"


def test_auto_sign_if_kms_skips_already_signed(tmp_path: Path) -> None:
    adapter = build_kms_adapter(tmp_path)
    result = {"signature_request_id": "SigReq_1", "status": "SIGNED"}
    out = adapter._auto_sign_if_kms(result)
    assert out["status"] == "SIGNED"


def test_auto_sign_if_kms_skips_empty_sig_id(tmp_path: Path) -> None:
    adapter = build_kms_adapter(tmp_path)
    result = {"signature_request_id": "", "status": "UNSIGNED"}
    out = adapter._auto_sign_if_kms(result)
    assert out["status"] == "UNSIGNED"


def test_sign_with_kms_signs_matching_messages(monkeypatch, tmp_path: Path) -> None:
    adapter = build_kms_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    test_message = "aa" * 32
    graphql_calls: list[dict[str, object]] = []

    responses = [
        # First call: get_signature_request_with_messages
        {
            "data": {
                "signatureRequest": {
                    "id": "SigReq_kms1",
                    "status": "UNSIGNED",
                    "messages": [
                        {"publicKey": FAKE_KMS_PUBKEY_HEX, "message": test_message},
                        {"publicKey": "02otherpubkey" + "00" * 27, "message": "bb" * 32},
                    ],
                }
            }
        },
        # Second call: signSignatureRequest
        {
            "data": {
                "signSignatureRequest": {
                    "signatureRequest": {"id": "SigReq_kms1", "status": "SIGNED"}
                }
            }
        },
    ]

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        body = json.loads(req.data.decode("utf-8"))
        graphql_calls.append(body)
        return FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    monkeypatch.setattr(
        "greenfloor.adapters.kms_signer.sign_digest",
        lambda key_id, region, msg_hex: "cc" * 64,
    )

    result = adapter.sign_with_kms(signature_request_id="SigReq_kms1")
    assert result["status"] == "SIGNED"

    # Should have made 2 GraphQL calls
    assert len(graphql_calls) == 2
    # Second call should be signSignatureRequest with our pubkey and message
    sign_vars = graphql_calls[1]["variables"]["input"]  # type: ignore[index]
    assert sign_vars["publicKey"] == FAKE_KMS_PUBKEY_HEX  # type: ignore[index]
    assert sign_vars["message"] == test_message  # type: ignore[index]
    assert sign_vars["signature"] == "cc" * 64  # type: ignore[index]


def test_sign_with_kms_skips_non_matching_messages(monkeypatch, tmp_path: Path) -> None:
    adapter = build_kms_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    responses = [
        {
            "data": {
                "signatureRequest": {
                    "id": "SigReq_nomatch",
                    "status": "UNSIGNED",
                    "messages": [
                        {"publicKey": "02otherpubkey" + "00" * 27, "message": "bb" * 32},
                    ],
                }
            }
        },
    ]

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        return FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    sign_digest_calls: list[str] = []
    monkeypatch.setattr(
        "greenfloor.adapters.kms_signer.sign_digest",
        lambda key_id, region, msg_hex: sign_digest_calls.append(msg_hex) or ("cc" * 64),
    )

    result = adapter.sign_with_kms(signature_request_id="SigReq_nomatch")
    assert result["status"] == "UNSIGNED"
    assert len(sign_digest_calls) == 0


def test_split_coins_with_kms_auto_signs(monkeypatch, tmp_path: Path) -> None:
    adapter = build_kms_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    test_message = "dd" * 32
    responses = [
        # splitCoins mutation response
        {
            "data": {
                "splitCoins": {"signatureRequest": {"id": "SigReq_split", "status": "UNSIGNED"}}
            }
        },
        # get_signature_request_with_messages
        {
            "data": {
                "signatureRequest": {
                    "id": "SigReq_split",
                    "status": "UNSIGNED",
                    "messages": [
                        {"publicKey": FAKE_KMS_PUBKEY_HEX, "message": test_message},
                    ],
                }
            }
        },
        # signSignatureRequest
        {
            "data": {
                "signSignatureRequest": {
                    "signatureRequest": {"id": "SigReq_split", "status": "SIGNED"}
                }
            }
        },
    ]

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        return FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    monkeypatch.setattr(
        "greenfloor.adapters.kms_signer.sign_digest",
        lambda key_id, region, msg_hex: "ee" * 64,
    )

    result = adapter.split_coins(
        coin_ids=["Coin_x"], amount_per_coin=100, number_of_coins=2, fee=500
    )
    assert result["signature_request_id"] == "SigReq_split"
    assert result["status"] == "SIGNED"


def test_create_offer_with_kms_auto_sign_wiring(monkeypatch, tmp_path: Path) -> None:
    adapter = build_kms_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    responses = [
        {
            "data": {
                "createOffer": {"signatureRequest": {"id": "SigReq_create", "status": "UNSIGNED"}}
            }
        }
    ]

    def _fake_urlopen(req, timeout=0):
        _ = req, timeout
        return FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    auto_sign_calls: list[dict[str, Any]] = []
    monkeypatch.setattr(
        adapter,
        "_auto_sign_if_kms",
        lambda result: (
            auto_sign_calls.append(dict(result))
            or {"signature_request_id": result["signature_request_id"], "status": "SIGNED"}
        ),
    )

    result = adapter.create_offer(
        offered=[{"assetId": "Asset_a", "amount": 100}],
        requested=[{"assetId": "Asset_b", "amount": 200}],
        fee=0,
        expires_at_iso="2026-01-01T00:00:00+00:00",
        split_input_coins=True,
        split_input_coins_fee=0,
    )
    assert result["signature_request_id"] == "SigReq_create"
    assert result["status"] == "SIGNED"
    assert auto_sign_calls == [{"signature_request_id": "SigReq_create", "status": "UNSIGNED"}]


def test_combine_coins_with_kms_auto_sign_wiring(monkeypatch, tmp_path: Path) -> None:
    adapter = build_kms_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    responses = [
        {
            "data": {
                "combineCoins": {"signatureRequest": {"id": "SigReq_combine", "status": "UNSIGNED"}}
            }
        }
    ]

    def _fake_urlopen(req, timeout=0):
        _ = req, timeout
        return FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    auto_sign_calls: list[dict[str, Any]] = []
    monkeypatch.setattr(
        adapter,
        "_auto_sign_if_kms",
        lambda result: (
            auto_sign_calls.append(dict(result))
            or {"signature_request_id": result["signature_request_id"], "status": "SIGNED"}
        ),
    )

    result = adapter.combine_coins(number_of_coins=2, fee=0)
    assert result["signature_request_id"] == "SigReq_combine"
    assert result["status"] == "SIGNED"
    assert auto_sign_calls == [{"signature_request_id": "SigReq_combine", "status": "UNSIGNED"}]


def test_cancel_offer_with_kms_auto_sign_wiring(monkeypatch, tmp_path: Path) -> None:
    adapter = build_kms_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    responses = [
        {
            "data": {
                "cancelOffer": {"signatureRequest": {"id": "SigReq_cancel", "status": "UNSIGNED"}}
            }
        }
    ]

    def _fake_urlopen(req, timeout=0):
        _ = req, timeout
        return FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    auto_sign_calls: list[dict[str, Any]] = []
    monkeypatch.setattr(
        adapter,
        "_auto_sign_if_kms",
        lambda result: (
            auto_sign_calls.append(dict(result))
            or {"signature_request_id": result["signature_request_id"], "status": "SIGNED"}
        ),
    )

    result = adapter.cancel_offer(offer_id="Offer_abc")
    assert result["signature_request_id"] == "SigReq_cancel"
    assert result["status"] == "SIGNED"
    assert auto_sign_calls == [{"signature_request_id": "SigReq_cancel", "status": "UNSIGNED"}]


def test_auto_sign_raises_kms_errors(monkeypatch, tmp_path: Path) -> None:
    adapter = build_kms_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _exploding_sign(**kwargs):
        raise RuntimeError("KMS unavailable")

    monkeypatch.setattr(adapter, "sign_with_kms", _exploding_sign)

    result = {"signature_request_id": "SigReq_fail", "status": "UNSIGNED"}
    with pytest.raises(RuntimeError, match="KMS unavailable"):
        adapter._auto_sign_if_kms(result)


def test_sign_with_kms_raises_without_key_id(tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    with pytest.raises(RuntimeError, match="kms_key_id is not configured"):
        adapter.sign_with_kms(signature_request_id="SigReq_1")


def test_sign_with_kms_handles_0x_prefixed_keys(monkeypatch, tmp_path: Path) -> None:
    """The API may return public keys with a 0x prefix; matching should still work."""
    adapter = build_kms_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    test_message = "ff" * 32
    responses = [
        {
            "data": {
                "signatureRequest": {
                    "id": "SigReq_prefix",
                    "status": "UNSIGNED",
                    "messages": [
                        {"publicKey": "0x" + FAKE_KMS_PUBKEY_HEX, "message": "0x" + test_message},
                    ],
                }
            }
        },
        {
            "data": {
                "signSignatureRequest": {
                    "signatureRequest": {"id": "SigReq_prefix", "status": "SIGNED"}
                }
            }
        },
    ]

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        return FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    monkeypatch.setattr(
        "greenfloor.adapters.kms_signer.sign_digest",
        lambda key_id, region, msg_hex: "ab" * 64,
    )

    result = adapter.sign_with_kms(signature_request_id="SigReq_prefix")
    assert result["status"] == "SIGNED"
