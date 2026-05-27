from __future__ import annotations

import io
import json
import logging
import urllib.error
from email.message import Message
from pathlib import Path
from typing import Any

import pytest

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig

from tests.helpers.cloud_wallet_adapter_fixtures import (
    FAKE_KMS_PUBKEY_HEX,
    FakeHttpResponse,
    build_adapter,
    build_kms_adapter,
    write_pem,
)

def test_cloud_wallet_adapter_rejects_pem_outside_dot_greenfloor(tmp_path: Path) -> None:
    pem_path = tmp_path / "outside.pem"
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
    with pytest.raises(
        ValueError, match="cloud_wallet_private_key_pem_path_must_be_under_dot_greenfloor"
    ):
        CloudWalletAdapter(
            CloudWalletConfig(
                base_url="https://wallet.example.com",
                user_key_id="key-1",
                private_key_pem_path=str(pem_path),
                vault_id="Wallet_123",
                network="mainnet",
            )
        )


def build_adapter(tmp_path: Path) -> CloudWalletAdapter:
    return CloudWalletAdapter(
        CloudWalletConfig(
            base_url="https://wallet.example.com",
            user_key_id="key-1",
            private_key_pem_path=str(write_pem(tmp_path)),
            vault_id="Wallet_123",
            network="mainnet",
        )
    )


def build_kms_adapter(tmp_path: Path) -> CloudWalletAdapter:
    """Build an adapter with KMS configured (public key pre-cached to avoid AWS call)."""
    return CloudWalletAdapter(
        CloudWalletConfig(
            base_url="https://wallet.example.com",
            user_key_id="key-1",
            private_key_pem_path=str(write_pem(tmp_path)),
            vault_id="Wallet_123",
            network="mainnet",
            kms_key_id="arn:aws:kms:us-west-2:123:key/fake",
            kms_region="us-west-2",
            kms_public_key_hex="03aabbccdd" + "00" * 28,
        )
    )


@pytest.mark.parametrize(
    ("src", "expected"),
    [
        ("query listCoins($w: ID!) { coins { edges { node { id } } } }", "query_listCoins"),
        ("mutation createOffer($input: CreateOfferInput!) { x }", "mutation_createOffer"),
        (
            "\nmutation SignSignatureRequest($input: SignSignatureRequestInput!) {\n  x\n}\n",
            "mutation_SignSignatureRequest",
        ),
        ("query { wallet { id } }", "query_anonymous"),
        ("subscription onBalance { event }", "subscription_onBalance"),
        ("not graphql at all", "unknown"),
    ],
)
def test_cloud_wallet_graphql_operation_label(src: str, expected: str) -> None:
    assert CloudWalletAdapter._graphql_operation_label(src) == expected
