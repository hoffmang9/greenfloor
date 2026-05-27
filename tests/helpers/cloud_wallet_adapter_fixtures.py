from __future__ import annotations

import json
from pathlib import Path

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig

FAKE_KMS_PUBKEY_HEX = "03aabbccdd" + "00" * 28


class FakeHttpResponse:
    def __init__(self, payload) -> None:
        self._payload = payload

    def read(self) -> bytes:
        return json.dumps(self._payload).encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        _ = exc_type, exc, tb
        return None


def write_pem(tmp_path: Path) -> Path:
    pem_path = tmp_path / ".greenfloor" / "keys" / "cloud-wallet-key.pem"
    pem_path.parent.mkdir(parents=True, exist_ok=True)
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
    return CloudWalletAdapter(
        CloudWalletConfig(
            base_url="https://wallet.example.com",
            user_key_id="key-1",
            private_key_pem_path=str(write_pem(tmp_path)),
            vault_id="Wallet_123",
            network="mainnet",
            kms_key_id="arn:aws:kms:us-west-2:123:key/fake",
            kms_region="us-west-2",
            kms_public_key_hex=FAKE_KMS_PUBKEY_HEX,
        )
    )
