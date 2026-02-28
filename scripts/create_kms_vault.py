#!/usr/bin/env python3
"""Create a vault on the ent-wallet API with a KMS P-256 custody key and a new BLS recovery key.

Usage:
    # Source the project .env then run:
    set -a; source .env; set +a
    .venv/bin/python scripts/create_kms_vault.py

Reads configuration from environment variables only (commonly loaded from .env).
This script does not read config/program.yaml.

Env vars (required -- set in .env):
    GREENFLOOR_CLOUD_WALLET_BASE_URL
        ent-wallet GraphQL API origin.
        Find at: https://vault.chia.net/settings.json -> GRAPHQL_URI
    GREENFLOOR_CLOUD_WALLET_USER_KEY_ID
        API key identifier.
        Find at: https://vault.chia.net -> Settings -> API Keys -> Key Id
    GREENFLOOR_CLOUD_WALLET_PRIVATE_KEY_PEM_PATH
        Local path to the downloaded API private key PEM file.
        Find at: https://vault.chia.net -> Settings -> API Keys -> download PEM
    GREENFLOOR_CLOUD_WALLET_KMS_KEY_ID
        AWS KMS key ARN for the P-256 custody key.
        Find at: AWS Console -> KMS -> Customer managed keys -> copy ARN
    GREENFLOOR_CLOUD_WALLET_KMS_REGION
        AWS region where the KMS key lives (e.g. us-west-2).

    AWS credentials -- via ~/.aws/credentials, AWS_PROFILE, or env vars.

Env vars (optional):
    GREENFLOOR_VAULT_NAME     -- name for the wallet (default: greenfloor-kms-vault)
    GREENFLOOR_RECOVERY_TIMELOCK -- recovery clawback in seconds (default: 1800 = 30 min)
"""

from __future__ import annotations

import json
import os
import sys
import textwrap

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig
from greenfloor.adapters.kms_signer import get_public_key_compressed_hex

DEFAULT_VAULT_NAME = "greenfloor-kms-vault"
DEFAULT_RECOVERY_TIMELOCK = 1800


def _require_env(name: str) -> str:
    val = os.environ.get(name, "").strip()
    if not val:
        print(f"ERROR: required env var {name} is not set", file=sys.stderr)
        sys.exit(1)
    return val


def _generate_bls_recovery_key() -> tuple[str, str]:
    """Generate a 24-word BLS mnemonic and return (mnemonic_words, public_key_hex)."""
    import chia_wallet_sdk as sdk  # type: ignore

    mnemonic = sdk.Mnemonic.generate(True)
    words = str(mnemonic)
    seed = mnemonic.to_seed("")
    master_sk = sdk.SecretKey.from_seed(seed)
    pubkey_bytes = master_sk.public_key().to_bytes()
    return words, pubkey_bytes.hex()


def _create_signer(
    adapter: CloudWalletAdapter,
    *,
    name: str,
    key_hex: str,
    curve: str,
) -> tuple[str, str]:
    """Create a signer via the ent-wallet API. Returns (signer_id, public_key_id)."""
    mutation = """
mutation CreateSigner($input: CreateSignerInput!) {
  createSigner(input: $input) {
    signer {
      id
      publicKeys {
        edges {
          node {
            id
          }
        }
      }
    }
  }
}
"""
    resp = adapter._graphql(
        query=mutation,
        variables={
            "input": {
                "name": name,
                "keys": [{"key": key_hex, "curve": curve}],
            }
        },
    )
    signer = (resp.get("createSigner") or {}).get("signer") or {}
    signer_id = signer.get("id", "")
    edges = (signer.get("publicKeys") or {}).get("edges") or []
    pk_id = ""
    if edges:
        node = edges[0].get("node") or {}
        pk_id = node.get("id", "")
    if not signer_id or not pk_id:
        raise RuntimeError(f"createSigner failed for {name}: {json.dumps(resp)}")
    return signer_id, pk_id


def _create_vault_wallet(
    adapter: CloudWalletAdapter,
    *,
    name: str,
    signer_ids: list[str],
    custody_threshold: int,
    custody_pk_ids: list[str],
    recovery_threshold: int,
    recovery_pk_ids: list[str],
    clawback_timelock: int,
) -> str:
    """Create a vault wallet via the ent-wallet API. Returns the wallet ID."""
    mutation = """
mutation CreateWallet($input: CreateWalletInput!) {
  createWallet(input: $input) {
    wallet {
      id
    }
  }
}
"""
    resp = adapter._graphql(
        query=mutation,
        variables={
            "input": {
                "name": name,
                "custodyConfig": {
                    "vaultCustodyConfig": {
                        "signerIds": signer_ids,
                        "custody": {
                            "threshold": custody_threshold,
                            "publicKeyIds": custody_pk_ids,
                        },
                        "recovery": {
                            "threshold": recovery_threshold,
                            "publicKeyIds": recovery_pk_ids,
                            "clawbackTimelock": clawback_timelock,
                        },
                    }
                },
                "watchtower": True,
            }
        },
    )
    wallet_id = ((resp.get("createWallet") or {}).get("wallet") or {}).get("id", "")
    if not wallet_id:
        raise RuntimeError(f"createWallet failed: {json.dumps(resp)}")
    return wallet_id


def main() -> None:
    base_url = _require_env("GREENFLOOR_CLOUD_WALLET_BASE_URL")
    user_key_id = _require_env("GREENFLOOR_CLOUD_WALLET_USER_KEY_ID")
    pem_path = _require_env("GREENFLOOR_CLOUD_WALLET_PRIVATE_KEY_PEM_PATH")

    kms_key_id = _require_env("GREENFLOOR_CLOUD_WALLET_KMS_KEY_ID")
    kms_region = _require_env("GREENFLOOR_CLOUD_WALLET_KMS_REGION")
    vault_name = os.environ.get("GREENFLOOR_VAULT_NAME", "").strip() or DEFAULT_VAULT_NAME
    recovery_timelock = int(
        os.environ.get("GREENFLOOR_RECOVERY_TIMELOCK", "").strip() or DEFAULT_RECOVERY_TIMELOCK
    )

    # Adapter with empty vault_id (no vault exists yet; only used for auth + _graphql)
    config = CloudWalletConfig(
        base_url=base_url,
        user_key_id=user_key_id,
        private_key_pem_path=pem_path,
        vault_id="",
        network="mainnet",
    )
    adapter = CloudWalletAdapter(config)

    # 1. Extract KMS P-256 public key
    print(f"Fetching P-256 public key from KMS key {kms_key_id} ...")
    custody_pubkey_hex = get_public_key_compressed_hex(kms_key_id, kms_region)
    print(f"  Compressed P-256 public key: {custody_pubkey_hex}")

    # 2. Generate 24-word BLS recovery mnemonic
    print("Generating 24-word BLS recovery mnemonic ...")
    mnemonic_words, recovery_pubkey_hex = _generate_bls_recovery_key()
    print(f"  BLS recovery public key: {recovery_pubkey_hex}")

    # 3. Create custody signer (KMS P-256)
    print("Creating custody signer (SECP256R1) ...")
    custody_signer_id, custody_pk_id = _create_signer(
        adapter, name=f"{vault_name}-custody-kms", key_hex=custody_pubkey_hex, curve="SECP256R1"
    )
    print(f"  Custody signer ID: {custody_signer_id}")
    print(f"  Custody public key ID: {custody_pk_id}")

    # 4. Create recovery signer (BLS)
    print("Creating recovery signer (BLS12_381) ...")
    recovery_signer_id, recovery_pk_id = _create_signer(
        adapter, name=f"{vault_name}-recovery-bls", key_hex=recovery_pubkey_hex, curve="BLS12_381"
    )
    print(f"  Recovery signer ID: {recovery_signer_id}")
    print(f"  Recovery public key ID: {recovery_pk_id}")

    # 5. Create vault wallet
    print(f"Creating vault wallet '{vault_name}' (recovery timelock: {recovery_timelock}s) ...")
    wallet_id = _create_vault_wallet(
        adapter,
        name=vault_name,
        signer_ids=[custody_signer_id, recovery_signer_id],
        custody_threshold=1,
        custody_pk_ids=[custody_pk_id],
        recovery_threshold=1,
        recovery_pk_ids=[recovery_pk_id],
        clawback_timelock=recovery_timelock,
    )
    print(f"  Vault wallet ID: {wallet_id}")

    # 6. Print summary
    print("\n" + "=" * 60)
    print("VAULT CREATION SUCCESSFUL")
    print("=" * 60)
    print(
        textwrap.dedent(f"""\
        Vault wallet ID : {wallet_id}
        KMS key ARN     : {kms_key_id}
        KMS region      : {kms_region}
        Custody pubkey  : {custody_pubkey_hex}
        Recovery timelock: {recovery_timelock}s ({recovery_timelock // 60} min)

        Add to your program.yaml cloud_wallet section:
          vault_id: "{wallet_id}"
          kms_key_id: "{kms_key_id}"
          kms_region: "{kms_region}"
          kms_public_key_hex: "{custody_pubkey_hex}"
    """)
    )

    print("=" * 60)
    print("RECOVERY MNEMONIC -- STORE THIS SECURELY AND OFFLINE")
    print("=" * 60)
    print(mnemonic_words)
    print("=" * 60)


if __name__ == "__main__":
    main()
