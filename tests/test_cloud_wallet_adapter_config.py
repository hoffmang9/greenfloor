from __future__ import annotations

from pathlib import Path

import pytest

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig


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
