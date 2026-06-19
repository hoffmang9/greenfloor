use chia_bls::PublicKey;
use chia_protocol::Bytes32;
use chia_puzzles::PREVENT_MULTIPLE_CREATE_COINS_HASH;
use chia_sdk_driver::{mips_puzzle_hash, MofN, Restriction, RestrictionKind};
use chia_sdk_types::{
    puzzles::{
        BlsMember, BlsMemberPuzzleAssert, Force1of2RestrictedVariable, K1Member,
        K1MemberPuzzleAssert, PasskeyMember, PasskeyMemberPuzzleAssert, PreventConditionOpcode,
        R1Member, R1MemberPuzzleAssert, SingletonMember, SingletonMemberWithMode, Timelock,
    },
    Mod,
};
use chia_secp::{K1PublicKey, R1PublicKey};
use clvm_utils::{tree_hash_atom, TreeHash};

use crate::error::{SignerError, SignerResult};

pub(crate) fn u32_to_usize(value: u32) -> SignerResult<usize> {
    usize::try_from(value).map_err(|_| SignerError::UnsupportedVaultThreshold)
}

#[derive(Debug, Clone, Default)]
pub struct MemberConfig {
    pub top_level: bool,
    pub nonce: u32,
    pub restrictions: Vec<Restriction>,
}

impl MemberConfig {
    pub fn with_top_level(&self, top_level: bool) -> Self {
        Self {
            top_level,
            ..self.clone()
        }
    }

    pub fn with_nonce(&self, nonce: u32) -> Self {
        Self {
            nonce,
            ..self.clone()
        }
    }

    pub fn with_restrictions(&self, restrictions: Vec<Restriction>) -> Self {
        Self {
            restrictions,
            ..self.clone()
        }
    }
}

pub fn m_of_n_hash(
    config: &MemberConfig,
    required: u32,
    items: Vec<TreeHash>,
) -> SignerResult<TreeHash> {
    let required_usize = u32_to_usize(required)?;
    member_hash(config, MofN::new(required_usize, items).inner_puzzle_hash())
}

pub fn custom_member_hash(config: &MemberConfig, inner_hash: TreeHash) -> SignerResult<TreeHash> {
    member_hash(config, inner_hash)
}

#[derive(Debug, Clone, Copy)]
pub struct P2ConditionsOrSingletonHashes {
    pub puzzle_hash: TreeHash,
    pub fixed_conditions_hash: TreeHash,
    pub p2_singleton_hash: TreeHash,
}

pub fn p2_conditions_or_singleton_puzzle_hash(
    fixed_delegated_puzzle_hash: TreeHash,
    launcher_id: Bytes32,
) -> SignerResult<P2ConditionsOrSingletonHashes> {
    let member_config = MemberConfig::default();
    let fixed_conditions_hash = custom_member_hash(&member_config, fixed_delegated_puzzle_hash)?;
    let p2_singleton_hash = singleton_member_hash(&member_config, launcher_id, false)?;
    let puzzle_hash = m_of_n_hash(
        &member_config.with_top_level(true),
        1,
        vec![fixed_conditions_hash, p2_singleton_hash],
    )?;
    Ok(P2ConditionsOrSingletonHashes {
        puzzle_hash,
        fixed_conditions_hash,
        p2_singleton_hash,
    })
}

pub fn r1_member_hash(
    config: &MemberConfig,
    public_key: R1PublicKey,
    fast_forward: bool,
) -> SignerResult<TreeHash> {
    member_hash(
        config,
        if fast_forward {
            R1MemberPuzzleAssert::new(public_key).curry_tree_hash()
        } else {
            R1Member::new(public_key).curry_tree_hash()
        },
    )
}

pub fn k1_member_hash(
    config: &MemberConfig,
    public_key: K1PublicKey,
    fast_forward: bool,
) -> SignerResult<TreeHash> {
    member_hash(
        config,
        if fast_forward {
            K1MemberPuzzleAssert::new(public_key).curry_tree_hash()
        } else {
            K1Member::new(public_key).curry_tree_hash()
        },
    )
}

pub fn bls_member_hash(
    config: &MemberConfig,
    public_key: PublicKey,
    fast_forward: bool,
) -> SignerResult<TreeHash> {
    member_hash(
        config,
        if fast_forward {
            BlsMemberPuzzleAssert::new(public_key).curry_tree_hash()
        } else {
            BlsMember::new(public_key).curry_tree_hash()
        },
    )
}

pub fn passkey_member_hash(
    config: &MemberConfig,
    public_key: R1PublicKey,
    fast_forward: bool,
) -> SignerResult<TreeHash> {
    member_hash(
        config,
        if fast_forward {
            PasskeyMemberPuzzleAssert::new(public_key).curry_tree_hash()
        } else {
            PasskeyMember::new(public_key).curry_tree_hash()
        },
    )
}

pub fn singleton_member_hash(
    config: &MemberConfig,
    launcher_id: Bytes32,
    fast_forward: bool,
) -> SignerResult<TreeHash> {
    member_hash(
        config,
        if fast_forward {
            SingletonMemberWithMode::new(launcher_id, 0b010_010).curry_tree_hash()
        } else {
            SingletonMember::new(launcher_id).curry_tree_hash()
        },
    )
}

/// Nonce member puzzle hash for vault singleton P2 discovery (top-level, no fast-forward).
pub fn singleton_member_puzzle_hash(launcher_id: Bytes32, nonce: u32) -> SignerResult<TreeHash> {
    singleton_member_hash(
        &MemberConfig::default()
            .with_top_level(true)
            .with_nonce(nonce),
        launcher_id,
        false,
    )
}

/// Hex-encoded nonce member puzzle hash for vault singleton P2 discovery.
pub fn singleton_member_puzzle_hash_hex(launcher_id: Bytes32, nonce: u32) -> SignerResult<String> {
    Ok(tree_hash_to_hex(singleton_member_puzzle_hash(
        launcher_id,
        nonce,
    )?))
}

/// Hex-encoded nonce member puzzle hash from a normalized launcher id string.
pub fn singleton_member_puzzle_hash_hex_from_launcher_id(
    launcher_id: &str,
    nonce: u32,
) -> SignerResult<String> {
    singleton_member_puzzle_hash_hex(hex_to_bytes32(launcher_id)?, nonce)
}

pub fn timelock_restriction(timelock: u64) -> Restriction {
    Restriction {
        kind: RestrictionKind::MemberCondition,
        puzzle_hash: Timelock::new(timelock).curry_tree_hash(),
    }
}

pub fn force_1_of_2_restriction(
    left_side_subtree_hash: Bytes32,
    nonce: u32,
    member_validator_list_hash: Bytes32,
    delegated_puzzle_validator_list_hash: Bytes32,
) -> SignerResult<Restriction> {
    Ok(Restriction {
        kind: RestrictionKind::DelegatedPuzzleWrapper,
        puzzle_hash: Force1of2RestrictedVariable::new(
            left_side_subtree_hash,
            u32_to_usize(nonce)?,
            member_validator_list_hash,
            delegated_puzzle_validator_list_hash,
        )
        .curry_tree_hash(),
    })
}

pub fn prevent_vault_side_effects_restriction() -> Vec<Restriction> {
    vec![
        prevent_condition_opcode_restriction(60),
        prevent_condition_opcode_restriction(62),
        prevent_condition_opcode_restriction(66),
        prevent_condition_opcode_restriction(67),
        prevent_multiple_create_coins_restriction(),
    ]
}

fn prevent_condition_opcode_restriction(condition_opcode: u16) -> Restriction {
    Restriction {
        kind: RestrictionKind::DelegatedPuzzleWrapper,
        puzzle_hash: PreventConditionOpcode::new(condition_opcode).curry_tree_hash(),
    }
}

fn prevent_multiple_create_coins_restriction() -> Restriction {
    Restriction {
        kind: RestrictionKind::DelegatedPuzzleWrapper,
        puzzle_hash: PREVENT_MULTIPLE_CREATE_COINS_HASH.into(),
    }
}

pub fn tree_hash_nil() -> TreeHash {
    tree_hash_atom(&[])
}

fn bytes33_from_vec(bytes: &[u8]) -> SignerResult<[u8; 33]> {
    if bytes.len() != 33 {
        return Err(SignerError::Other(format!(
            "expected 33-byte key, got {} bytes",
            bytes.len()
        )));
    }
    let mut out = [0u8; 33];
    out.copy_from_slice(bytes);
    Ok(out)
}

fn bytes48_from_vec(bytes: &[u8]) -> SignerResult<[u8; 48]> {
    if bytes.len() != 48 {
        return Err(SignerError::Other(format!(
            "expected 48-byte key, got {} bytes",
            bytes.len()
        )));
    }
    let mut out = [0u8; 48];
    out.copy_from_slice(bytes);
    Ok(out)
}

fn member_hash(config: &MemberConfig, inner_hash: TreeHash) -> SignerResult<TreeHash> {
    Ok(mips_puzzle_hash(
        u32_to_usize(config.nonce)?,
        config.restrictions.clone(),
        inner_hash,
        config.top_level,
    ))
}

#[derive(Debug, Clone)]
pub struct WalletKey {
    pub public_key_hex: String,
    pub curve: String,
}

pub fn member_hash_for_key(config: &MemberConfig, key: &WalletKey) -> SignerResult<TreeHash> {
    let curve = key.curve.trim().to_ascii_uppercase();
    let key_bytes = hex_to_bytes(&key.public_key_hex)?;
    match curve.as_str() {
        "SECP256R1" => {
            let key_array = bytes33_from_vec(&key_bytes)?;
            let pk = R1PublicKey::from_bytes(&key_array).map_err(|err| {
                SignerError::UnsupportedVaultCurve(format!("SECP256R1 decode: {err}"))
            })?;
            Ok(r1_member_hash(config, pk, true)?)
        }
        "SECP256K1" => {
            let key_array = bytes33_from_vec(&key_bytes)?;
            let pk = K1PublicKey::from_bytes(&key_array).map_err(|err| {
                SignerError::UnsupportedVaultCurve(format!("SECP256K1 decode: {err}"))
            })?;
            Ok(k1_member_hash(config, pk, true)?)
        }
        "WEBAUTHN" => {
            let key_array = bytes33_from_vec(&key_bytes)?;
            let pk = R1PublicKey::from_bytes(&key_array).map_err(|err| {
                SignerError::UnsupportedVaultCurve(format!("WEBAUTHN decode: {err}"))
            })?;
            Ok(passkey_member_hash(config, pk, true)?)
        }
        "BLS12_381" => {
            let key_array = bytes48_from_vec(&key_bytes)?;
            let pk = PublicKey::from_bytes(&key_array).map_err(|err| {
                SignerError::UnsupportedVaultCurve(format!("BLS12_381 decode: {err}"))
            })?;
            Ok(bls_member_hash(config, pk, false)?)
        }
        other => Err(SignerError::UnsupportedVaultCurve(other.to_string())),
    }
}

pub fn hex_to_bytes32(value: &str) -> SignerResult<Bytes32> {
    let bytes = hex_to_bytes(value)?;
    if bytes.len() != 32 {
        return Err(SignerError::Other(format!(
            "expected 32-byte hex value, got {} bytes",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(Bytes32::new(out))
}

pub fn hex_to_bytes(value: &str) -> SignerResult<Vec<u8>> {
    let normalized = crate::kms::normalize_hex(value);
    if normalized.is_empty() || !normalized.len().is_multiple_of(2) {
        return Err(SignerError::Other(format!("invalid hex: {value}")));
    }
    hex::decode(normalized).map_err(|err| SignerError::Other(format!("invalid hex: {err}")))
}

pub fn tree_hash_to_hex(hash: TreeHash) -> String {
    hex::encode(hash.to_bytes())
}

pub fn bytes32_to_hex(value: Bytes32) -> String {
    hex::encode(value.to_bytes())
}

#[cfg(test)]
mod tests {
    use chia_protocol::Bytes32;

    use super::{
        hex_to_bytes32, singleton_member_puzzle_hash, singleton_member_puzzle_hash_hex,
        singleton_member_puzzle_hash_hex_from_launcher_id,
    };

    #[test]
    fn singleton_member_puzzle_hash_hex_matches_tree_hash_hex() {
        let launcher = Bytes32::new([0x44; 32]);
        let hash = singleton_member_puzzle_hash(launcher, 3).expect("hash");
        let hex = singleton_member_puzzle_hash_hex(launcher, 3).expect("hex");
        assert_eq!(hex.len(), 64);
        assert_eq!(hex, super::tree_hash_to_hex(hash));
    }

    #[test]
    fn singleton_member_puzzle_hash_hex_from_launcher_id_normalizes_input() {
        let launcher = "ab".repeat(32);
        let hex = singleton_member_puzzle_hash_hex_from_launcher_id(&launcher, 0).expect("hex");
        let bytes = hex_to_bytes32(&hex).expect("bytes");
        let direct = singleton_member_puzzle_hash_hex(bytes, 0).expect("direct");
        assert_eq!(hex, direct);
    }

    #[test]
    fn singleton_member_puzzle_hash_changes_with_nonce() {
        let launcher = Bytes32::new([0x55; 32]);
        let nonce0 = singleton_member_puzzle_hash_hex(launcher, 0).expect("nonce0");
        let nonce1 = singleton_member_puzzle_hash_hex(launcher, 1).expect("nonce1");
        assert_ne!(nonce0, nonce1);
    }
}
