from __future__ import annotations

import pytest

# Golden vectors shared with greenfloor-signer/src/test_support/golden.rs
LAUNCHER_ID_HEX = "aa" * 32
CUSTODY_KEY_HEX = "02" * 33
RECOVERY_KEY_HEX = (
    "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363"
    "b2d726218135b25b814f94df4749fc58"
)
INNER_PUZZLE_HASH_HEX = "c0c282903488033a205e05e42546471e140d3d2c29099588465d0e93c5a11902"
P2_SINGLETON_MESSAGE_HASH_HEX = (
    "4141f038995622a43f2d567b8011c43819c81085066b143d942e990b8036cf6c"
)
CUSTODY_HASH_HEX = "a0b54784e43c1a53dac6ff8855b28741470df65399a9a6cafbb80c046e4c487c"
RECOVERY_HASH_HEX = "dcea66a7f4d21d7dfa01b5c8d4cdf1d7df4c53d3b0532ba03f0dd0ecab629107"


def _require_sdk():
    try:
        import chia_wallet_sdk as sdk  # type: ignore
    except Exception:
        pytest.skip("chia_wallet_sdk import unavailable")
    return sdk


def _compute_vault_hashes() -> dict[str, bytes]:
    sdk = _require_sdk()
    clvm = sdk.Clvm()
    member_config = sdk.MemberConfig()
    launcher_id = bytes.fromhex(LAUNCHER_ID_HEX)
    custody_key = bytes.fromhex(CUSTODY_KEY_HEX)
    recovery_key = bytes.fromhex(RECOVERY_KEY_HEX)

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


def test_greenfloor_signer_vault_hash_parity_matches_python_sdk() -> None:
    hashes = _compute_vault_hashes()
    assert hashes["inner_puzzle_hash"].hex() == INNER_PUZZLE_HASH_HEX
    assert hashes["p2_singleton_message_hash"].hex() == P2_SINGLETON_MESSAGE_HASH_HEX
    assert hashes["custody_hash"].hex() == CUSTODY_HASH_HEX
    assert hashes["recovery_hash"].hex() == RECOVERY_HASH_HEX
