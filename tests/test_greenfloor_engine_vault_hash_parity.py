from __future__ import annotations

import json
from pathlib import Path

import pytest

# Expected hash vectors are canonical in tests/fixtures/vault_hash_golden.json
# (mirrored by greenfloor-engine/src/test_support/golden.rs during migration).
FIXTURE_PATH = Path(__file__).resolve().parent / "fixtures" / "vault_hash_golden.json"


def _load_golden_fixture() -> dict[str, str]:
    with FIXTURE_PATH.open(encoding="utf-8") as handle:
        return json.load(handle)


def _require_sdk():
    try:
        import chia_wallet_sdk as sdk  # type: ignore
    except Exception:
        pytest.skip("chia_wallet_sdk import unavailable")
    return sdk


def _compute_vault_hashes(fixture: dict[str, str]) -> dict[str, bytes]:
    sdk = _require_sdk()
    clvm = sdk.Clvm()
    member_config = sdk.MemberConfig()
    launcher_id = bytes.fromhex(fixture["launcher_id"])
    custody_key = bytes.fromhex(fixture["custody_key"])
    recovery_key = bytes.fromhex(fixture["recovery_key"])

    custody_hash = bytes(
        sdk.r1_member_hash(member_config, sdk.R1PublicKey.from_bytes(custody_key), True)
    )
    timelock = sdk.timelock_restriction(3600)
    member_validator_list_hash = sdk.tree_hash_pair(
        timelock.puzzle_hash,
        clvm.nil().tree_hash(),
    )
    recovery_restrictions = [
        sdk.force_1_of_2_restriction(
            custody_hash,
            0,
            member_validator_list_hash,
            clvm.nil().tree_hash(),
        ),
        *sdk.prevent_vault_side_effects_restriction(),
    ]
    recovery_config = member_config.with_restrictions(recovery_restrictions)
    recovery_hash = bytes(
        sdk.bls_member_hash(
            recovery_config,
            sdk.PublicKey.from_bytes(recovery_key),
            False,
        )
    )
    inner_puzzle_hash = bytes(
        sdk.m_of_n_hash(
            member_config.with_top_level(True),
            1,
            [custody_hash, recovery_hash],
        )
    )
    p2_singleton_message_hash = bytes(
        sdk.singleton_member_hash(
            sdk.MemberConfig().with_top_level(True),
            launcher_id,
            False,
        )
    )
    return {
        "inner_puzzle_hash": inner_puzzle_hash,
        "p2_singleton_message_hash": p2_singleton_message_hash,
        "custody_hash": custody_hash,
        "recovery_hash": recovery_hash,
    }


def test_greenfloor_engine_vault_hash_parity_matches_python_sdk() -> None:
    fixture = _load_golden_fixture()
    hashes = _compute_vault_hashes(fixture)
    assert hashes["inner_puzzle_hash"].hex() == fixture["inner_puzzle_hash"]
    assert hashes["p2_singleton_message_hash"].hex() == fixture["p2_singleton_message_hash"]
    assert hashes["custody_hash"].hex() == fixture["custody_hash"]
    assert hashes["recovery_hash"].hex() == fixture["recovery_hash"]
