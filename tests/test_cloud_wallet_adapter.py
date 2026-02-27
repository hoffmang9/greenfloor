from __future__ import annotations

import io
import json
import urllib.error
from email.message import Message
from pathlib import Path

import pytest

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig


class _FakeHttpResponse:
    def __init__(self, payload) -> None:
        self._payload = payload

    def read(self) -> bytes:
        return json.dumps(self._payload).encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        _ = exc_type, exc, tb
        return None


def _write_pem(tmp_path: Path) -> Path:
    pem_path = tmp_path / "cloud-wallet-key.pem"
    pem_path.write_text(
        "\n".join(
            [
                "-----BEGIN PRIVATE KEY-----",
                "not-a-real-key",
                "-----END PRIVATE KEY-----",
            ]
        ),
        encoding="utf-8",
    )
    return pem_path


def _build_adapter(tmp_path: Path) -> CloudWalletAdapter:
    return CloudWalletAdapter(
        CloudWalletConfig(
            base_url="https://wallet.example.com",
            user_key_id="key-1",
            private_key_pem_path=str(_write_pem(tmp_path)),
            vault_id="Wallet_123",
            network="mainnet",
        )
    )


def _build_kms_adapter(tmp_path: Path) -> CloudWalletAdapter:
    """Build an adapter with KMS configured (public key pre-cached to avoid AWS call)."""
    return CloudWalletAdapter(
        CloudWalletConfig(
            base_url="https://wallet.example.com",
            user_key_id="key-1",
            private_key_pem_path=str(_write_pem(tmp_path)),
            vault_id="Wallet_123",
            network="mainnet",
            kms_key_id="arn:aws:kms:us-west-2:123:key/fake",
            kms_region="us-west-2",
            kms_public_key_hex="03aabbccdd" + "00" * 28,
        )
    )


def test_cloud_wallet_list_coins_paginates_and_accumulates(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    responses = [
        {
            "data": {
                "coins": {
                    "pageInfo": {"hasNextPage": True, "endCursor": "cursor-1"},
                    "edges": [
                        {"node": {"id": "Coin_1", "name": "11", "amount": 10, "state": "CONFIRMED"}}
                    ],
                }
            }
        },
        {
            "data": {
                "coins": {
                    "pageInfo": {"hasNextPage": False, "endCursor": ""},
                    "edges": [
                        {
                            "node": {
                                "id": "Coin_2",
                                "name": "22",
                                "amount": 20,
                                "state": "CONFIRMED",
                            }
                        },
                        {"node": "not-a-dict"},
                    ],
                }
            }
        },
    ]

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return _FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    coins = adapter.list_coins()
    assert [c["id"] for c in coins] == ["Coin_1", "Coin_2"]


def test_cloud_wallet_list_coins_stops_on_missing_end_cursor(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    calls = {"n": 0}

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        calls["n"] += 1
        return _FakeHttpResponse(
            {
                "data": {
                    "coins": {
                        "pageInfo": {"hasNextPage": True, "endCursor": ""},
                        "edges": [{"node": {"id": "Coin_1", "name": "11", "amount": 10}}],
                    }
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    coins = adapter.list_coins()
    assert len(coins) == 1
    assert calls["n"] == 1


def test_cloud_wallet_graphql_http_error_contains_status_and_snippet(
    monkeypatch, tmp_path: Path
) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        raise urllib.error.HTTPError(
            req.full_url,
            500,
            "server error",
            Message(),
            io.BytesIO(b'{"error":"boom"}'),
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(RuntimeError, match=r"cloud_wallet_http_error:500:"):
        adapter._graphql(query="query test {}", variables={})


def test_cloud_wallet_graphql_network_error_is_classified(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        raise urllib.error.URLError("offline")

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(RuntimeError, match=r"cloud_wallet_network_error:offline"):
        adapter._graphql(query="query test {}", variables={})


def test_cloud_wallet_graphql_error_payload_raises(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return _FakeHttpResponse({"errors": [{"message": "bad request"}]})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(RuntimeError, match=r"cloud_wallet_graphql_error:bad request"):
        adapter._graphql(query="query test {}", variables={})


def test_cloud_wallet_graphql_missing_data_raises(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return _FakeHttpResponse({"data": []})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(RuntimeError, match=r"cloud_wallet_missing_data"):
        adapter._graphql(query="query test {}", variables={})


def test_cloud_wallet_get_signature_request_handles_non_dict(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return _FakeHttpResponse({"data": {"signatureRequest": "invalid-shape"}})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.get_signature_request(signature_request_id="SignatureRequest_1")
    assert payload == {"id": "SignatureRequest_1", "status": "UNKNOWN"}


def test_cloud_wallet_cancel_offer_returns_signature_request(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return _FakeHttpResponse(
            {
                "data": {
                    "cancelOffer": {
                        "signatureRequest": {
                            "id": "SignatureRequest_cancel_1",
                            "status": "SUBMITTED",
                        }
                    }
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.cancel_offer(offer_id="Offer_abc")
    assert payload == {
        "signature_request_id": "SignatureRequest_cancel_1",
        "status": "SUBMITTED",
    }
    variables = captured["body"]["variables"]["input"]  # type: ignore[index]
    assert variables["offerId"] == "Offer_abc"  # type: ignore[index]
    assert variables["cancelOffChain"] is False  # type: ignore[index]


def test_cloud_wallet_cancel_offer_off_chain_sets_flag(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return _FakeHttpResponse({"data": {"cancelOffer": {"signatureRequest": None}}})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.cancel_offer(offer_id="Offer_pending", cancel_off_chain=True)
    assert payload == {"signature_request_id": "", "status": ""}
    variables = captured["body"]["variables"]["input"]  # type: ignore[index]
    assert variables["offerId"] == "Offer_pending"  # type: ignore[index]
    assert variables["cancelOffChain"] is True  # type: ignore[index]


def test_cloud_wallet_create_offer_includes_split_input_coin_options(
    monkeypatch, tmp_path: Path
) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return _FakeHttpResponse(
            {
                "data": {
                    "createOffer": {
                        "signatureRequest": {
                            "id": "SignatureRequest_create_1",
                            "status": "SUBMITTED",
                        }
                    }
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.create_offer(
        offered=[{"assetId": "Asset_a", "amount": 1}],
        requested=[{"assetId": "Asset_b", "amount": 2}],
        fee=0,
        expires_at_iso="2026-02-26T00:00:00+00:00",
        split_input_coins=True,
        split_input_coins_fee=0,
    )
    assert payload == {
        "signature_request_id": "SignatureRequest_create_1",
        "status": "SUBMITTED",
    }
    variables = captured["body"]["variables"]["input"]  # type: ignore[index]
    assert variables["splitInputCoins"] is True  # type: ignore[index]
    assert variables["splitInputCoinsFee"] == 0  # type: ignore[index]


def test_cloud_wallet_get_wallet_passes_offer_filters(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return _FakeHttpResponse(
            {
                "data": {
                    "wallet": {
                        "offers": {
                            "edges": [
                                {
                                    "node": {
                                        "id": "WalletOffer_1",
                                        "offerId": "Offer_1",
                                        "state": "OPEN",
                                        "settlementType": "UNSIGNED",
                                        "expiresAt": None,
                                        "bech32": "offer1abc",
                                        "createdAt": "2026-02-26T00:00:00+00:00",
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.get_wallet(is_creator=True, states=["OPEN", "PENDING"], first=25)
    assert len(payload["offers"]) == 1
    variables = captured["body"]["variables"]  # type: ignore[index]
    assert variables["isCreator"] is True  # type: ignore[index]
    assert variables["states"] == ["OPEN", "PENDING"]  # type: ignore[index]
    assert variables["first"] == 25  # type: ignore[index]


# ---------------------------------------------------------------------------
# split_coins / combine_coins
# ---------------------------------------------------------------------------


def test_cloud_wallet_split_coins_sends_correct_variables(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return _FakeHttpResponse(
            {
                "data": {
                    "splitCoins": {"signatureRequest": {"id": "SigReq_s1", "status": "UNSIGNED"}}
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    result = adapter.split_coins(
        coin_ids=["Coin_a", "Coin_b"],
        amount_per_coin=100,
        number_of_coins=5,
        fee=500,
    )
    assert result["signature_request_id"] == "SigReq_s1"
    assert result["status"] == "UNSIGNED"
    variables = captured["body"]["variables"]  # type: ignore[index]
    assert variables["coinIds"] == ["Coin_a", "Coin_b"]
    assert variables["amountPerCoin"] == 100
    assert variables["numberOfCoins"] == 5
    assert variables["fee"] == 500
    assert variables["walletId"] == "Wallet_123"


def test_cloud_wallet_combine_coins_sends_correct_variables(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return _FakeHttpResponse(
            {
                "data": {
                    "combineCoins": {"signatureRequest": {"id": "SigReq_c1", "status": "SUBMITTED"}}
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    result = adapter.combine_coins(
        number_of_coins=3,
        fee=1000,
        largest_first=False,
        asset_id="Asset_xyz",
        input_coin_ids=["Coin_1", "Coin_2"],
        target_amount=500,
    )
    assert result["signature_request_id"] == "SigReq_c1"
    assert result["status"] == "SUBMITTED"
    variables = captured["body"]["variables"]  # type: ignore[index]
    assert variables["numberOfCoins"] == 3
    assert variables["fee"] == 1000
    assert variables["largestFirst"] is False
    assert variables["assetId"] == "Asset_xyz"
    assert variables["inputCoinIds"] == ["Coin_1", "Coin_2"]
    assert variables["targetAmount"] == 500


def test_cloud_wallet_combine_coins_optional_fields_default_none(
    monkeypatch, tmp_path: Path
) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return _FakeHttpResponse(
            {
                "data": {
                    "combineCoins": {"signatureRequest": {"id": "SigReq_c2", "status": "UNSIGNED"}}
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    result = adapter.combine_coins(number_of_coins=2, fee=0)
    assert result["signature_request_id"] == "SigReq_c2"
    variables = captured["body"]["variables"]  # type: ignore[index]
    assert variables["targetAmount"] is None
    assert variables["inputCoinIds"] is None
    assert variables["assetId"] is None


# ---------------------------------------------------------------------------
# KMS signing integration
# ---------------------------------------------------------------------------

_FAKE_KMS_PUBKEY_HEX = "03aabbccdd" + "00" * 28


def test_kms_configured_returns_false_without_kms(tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    assert adapter.kms_configured is False


def test_kms_configured_returns_true_with_kms(tmp_path: Path) -> None:
    adapter = _build_kms_adapter(tmp_path)
    assert adapter.kms_configured is True


def test_auto_sign_if_kms_is_noop_without_kms(tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    result = {"signature_request_id": "SigReq_1", "status": "UNSIGNED"}
    out = adapter._auto_sign_if_kms(result)
    assert out["status"] == "UNSIGNED"


def test_auto_sign_if_kms_skips_already_signed(tmp_path: Path) -> None:
    adapter = _build_kms_adapter(tmp_path)
    result = {"signature_request_id": "SigReq_1", "status": "SIGNED"}
    out = adapter._auto_sign_if_kms(result)
    assert out["status"] == "SIGNED"


def test_auto_sign_if_kms_skips_empty_sig_id(tmp_path: Path) -> None:
    adapter = _build_kms_adapter(tmp_path)
    result = {"signature_request_id": "", "status": "UNSIGNED"}
    out = adapter._auto_sign_if_kms(result)
    assert out["status"] == "UNSIGNED"


def test_sign_with_kms_signs_matching_messages(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_kms_adapter(tmp_path)
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
                        {"publicKey": _FAKE_KMS_PUBKEY_HEX, "message": test_message},
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
        return _FakeHttpResponse(responses.pop(0))

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
    assert sign_vars["publicKey"] == _FAKE_KMS_PUBKEY_HEX  # type: ignore[index]
    assert sign_vars["message"] == test_message  # type: ignore[index]
    assert sign_vars["signature"] == "cc" * 64  # type: ignore[index]


def test_sign_with_kms_skips_non_matching_messages(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_kms_adapter(tmp_path)
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
        return _FakeHttpResponse(responses.pop(0))

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
    adapter = _build_kms_adapter(tmp_path)
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
                        {"publicKey": _FAKE_KMS_PUBKEY_HEX, "message": test_message},
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
        return _FakeHttpResponse(responses.pop(0))

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


def test_auto_sign_catches_kms_errors(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_kms_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _exploding_sign(**kwargs):
        raise RuntimeError("KMS unavailable")

    monkeypatch.setattr(adapter, "sign_with_kms", _exploding_sign)

    result = {"signature_request_id": "SigReq_fail", "status": "UNSIGNED"}
    out = adapter._auto_sign_if_kms(result)
    # Should not raise; status unchanged
    assert out["status"] == "UNSIGNED"


def test_sign_with_kms_raises_without_key_id(tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    with pytest.raises(RuntimeError, match="kms_key_id is not configured"):
        adapter.sign_with_kms(signature_request_id="SigReq_1")


def test_sign_with_kms_handles_0x_prefixed_keys(monkeypatch, tmp_path: Path) -> None:
    """The API may return public keys with a 0x prefix; matching should still work."""
    adapter = _build_kms_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    test_message = "ff" * 32
    responses = [
        {
            "data": {
                "signatureRequest": {
                    "id": "SigReq_prefix",
                    "status": "UNSIGNED",
                    "messages": [
                        {"publicKey": "0x" + _FAKE_KMS_PUBKEY_HEX, "message": "0x" + test_message},
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
        return _FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    monkeypatch.setattr(
        "greenfloor.adapters.kms_signer.sign_digest",
        lambda key_id, region, msg_hex: "ab" * 64,
    )

    result = adapter.sign_with_kms(signature_request_id="SigReq_prefix")
    assert result["status"] == "SIGNED"
