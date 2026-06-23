use chia_bls::PublicKey;
use chia_protocol::Bytes32;
use chia_sdk_types::{
    puzzles::{
        BlsMember, BlsMemberPuzzleAssert, K1Member, K1MemberPuzzleAssert, PasskeyMember,
        PasskeyMemberPuzzleAssert, R1Member, R1MemberPuzzleAssert, SingletonMember,
        SingletonMemberWithMode,
    },
    Mod,
};
use chia_secp::{K1PublicKey, R1PublicKey};
use clvm_utils::TreeHash;

use crate::error::{SignerError, SignerResult};
use crate::hex::{fixed_bytes, hex_to_bytes};

use super::config::{MemberConfig, WalletKey};
use super::hash::member_hash;

fn member_hash_fast_forward(
    config: &MemberConfig,
    fast_forward: bool,
    normal: TreeHash,
    assert: TreeHash,
) -> SignerResult<TreeHash> {
    member_hash(config, if fast_forward { assert } else { normal })
}

/// R1 member hash.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn r1_member_hash(
    config: &MemberConfig,
    public_key: R1PublicKey,
    fast_forward: bool,
) -> SignerResult<TreeHash> {
    member_hash_fast_forward(
        config,
        fast_forward,
        R1Member::new(public_key).curry_tree_hash(),
        R1MemberPuzzleAssert::new(public_key).curry_tree_hash(),
    )
}

/// K1 member hash.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn k1_member_hash(
    config: &MemberConfig,
    public_key: K1PublicKey,
    fast_forward: bool,
) -> SignerResult<TreeHash> {
    member_hash_fast_forward(
        config,
        fast_forward,
        K1Member::new(public_key).curry_tree_hash(),
        K1MemberPuzzleAssert::new(public_key).curry_tree_hash(),
    )
}

/// Bls member hash.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn bls_member_hash(
    config: &MemberConfig,
    public_key: PublicKey,
    fast_forward: bool,
) -> SignerResult<TreeHash> {
    member_hash_fast_forward(
        config,
        fast_forward,
        BlsMember::new(public_key).curry_tree_hash(),
        BlsMemberPuzzleAssert::new(public_key).curry_tree_hash(),
    )
}

/// Passkey member hash.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn passkey_member_hash(
    config: &MemberConfig,
    public_key: R1PublicKey,
    fast_forward: bool,
) -> SignerResult<TreeHash> {
    member_hash_fast_forward(
        config,
        fast_forward,
        PasskeyMember::new(public_key).curry_tree_hash(),
        PasskeyMemberPuzzleAssert::new(public_key).curry_tree_hash(),
    )
}

/// Singleton member hash.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn singleton_member_hash(
    config: &MemberConfig,
    launcher_id: Bytes32,
    fast_forward: bool,
) -> SignerResult<TreeHash> {
    member_hash_fast_forward(
        config,
        fast_forward,
        SingletonMember::new(launcher_id).curry_tree_hash(),
        SingletonMemberWithMode::new(launcher_id, 0b010_010).curry_tree_hash(),
    )
}

#[derive(Clone, Copy)]
enum WalletCurve {
    Secp256R1,
    Secp256K1,
    WebAuthn,
    Bls12_381,
}

impl WalletCurve {
    fn parse(curve: &str) -> Option<Self> {
        match curve.trim().to_ascii_uppercase().as_str() {
            "SECP256R1" => Some(Self::Secp256R1),
            "SECP256K1" => Some(Self::Secp256K1),
            "WEBAUTHN" => Some(Self::WebAuthn),
            "BLS12_381" => Some(Self::Bls12_381),
            _ => None,
        }
    }

    fn hash_key(self, config: &MemberConfig, key_bytes: &[u8]) -> SignerResult<TreeHash> {
        match self {
            Self::Secp256R1 => {
                let pk = decode_r1_public_key(key_bytes, "SECP256R1")?;
                r1_member_hash(config, pk, true)
            }
            Self::Secp256K1 => {
                let pk = decode_k1_public_key(key_bytes)?;
                k1_member_hash(config, pk, true)
            }
            Self::WebAuthn => {
                let pk = decode_r1_public_key(key_bytes, "WEBAUTHN")?;
                passkey_member_hash(config, pk, true)
            }
            Self::Bls12_381 => {
                let key_array = fixed_bytes::<48>(key_bytes)?;
                let pk = PublicKey::from_bytes(&key_array).map_err(|err| {
                    SignerError::UnsupportedVaultCurve(format!("BLS12_381 decode: {err}"))
                })?;
                bls_member_hash(config, pk, false)
            }
        }
    }
}

fn decode_r1_public_key(key_bytes: &[u8], curve_label: &str) -> SignerResult<R1PublicKey> {
    let key_array = fixed_bytes::<33>(key_bytes)?;
    R1PublicKey::from_bytes(&key_array)
        .map_err(|err| SignerError::UnsupportedVaultCurve(format!("{curve_label} decode: {err}")))
}

fn decode_k1_public_key(key_bytes: &[u8]) -> SignerResult<K1PublicKey> {
    let key_array = fixed_bytes::<33>(key_bytes)?;
    K1PublicKey::from_bytes(&key_array)
        .map_err(|err| SignerError::UnsupportedVaultCurve(format!("SECP256K1 decode: {err}")))
}

/// Member hash for key.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn member_hash_for_key(config: &MemberConfig, key: &WalletKey) -> SignerResult<TreeHash> {
    let Some(curve) = WalletCurve::parse(&key.curve) else {
        return Err(SignerError::UnsupportedVaultCurve(key.curve.clone()));
    };
    let key_bytes = hex_to_bytes(&key.public_key_hex)?;
    curve.hash_key(config, &key_bytes)
}

#[cfg(test)]
mod tests {
    use chia_sdk_test::{BlsPair, K1Pair, R1Pair};

    use super::*;
    use crate::hex::hex_to_bytes32;
    use crate::hex::tree_hash_to_hex;
    use crate::test_support::golden::{golden_snapshot, CUSTODY_HASH_HEX, LAUNCHER_ID_HEX};

    fn wallet_key(curve: &str, public_key_hex: &str) -> WalletKey {
        WalletKey {
            curve: curve.to_string(),
            public_key_hex: public_key_hex.to_string(),
        }
    }

    #[test]
    fn member_hash_for_key_matches_golden_custody_vector() {
        let snapshot = golden_snapshot();
        let hash = member_hash_for_key(&MemberConfig::default(), &snapshot.custody_keys[0])
            .expect("custody hash");
        assert_eq!(tree_hash_to_hex(hash), CUSTODY_HASH_HEX);
    }

    #[test]
    fn member_hash_for_key_supports_all_wallet_curves() {
        let config = MemberConfig::default();
        let r1 = R1Pair::new(7);
        let k1 = K1Pair::new(8);
        let bls = BlsPair::new(9);
        let passkey = R1Pair::new(10);

        let r1_hash = member_hash_for_key(
            &config,
            &wallet_key("SECP256R1", &hex::encode(r1.pk.to_bytes())),
        )
        .expect("r1");
        let k1_hash = member_hash_for_key(
            &config,
            &wallet_key("SECP256K1", &hex::encode(k1.pk.to_bytes())),
        )
        .expect("k1");
        let bls_hash = member_hash_for_key(
            &config,
            &wallet_key("BLS12_381", &hex::encode(bls.pk.to_bytes())),
        )
        .expect("bls");
        let passkey_hash = member_hash_for_key(
            &config,
            &wallet_key("WEBAUTHN", &hex::encode(passkey.pk.to_bytes())),
        )
        .expect("passkey");
        assert_ne!(r1_hash, k1_hash);
        assert_ne!(r1_hash, bls_hash);
        assert_ne!(passkey_hash, r1_hash);

        let launcher_id = hex_to_bytes32(LAUNCHER_ID_HEX).expect("launcher");
        let normal = singleton_member_hash(&config, launcher_id, false).expect("normal");
        let fast_forward = singleton_member_hash(&config, launcher_id, true).expect("ff");
        assert_ne!(normal, fast_forward);
    }

    #[test]
    fn member_hash_for_key_rejects_unknown_curve() {
        let config = MemberConfig::default();
        let err = member_hash_for_key(&config, &wallet_key("ED25519", &hex::encode([0u8; 32])))
            .expect_err("unsupported curve");
        assert!(matches!(err, SignerError::UnsupportedVaultCurve(_)));
    }

    #[test]
    fn fast_forward_flag_changes_r1_member_hash() {
        let config = MemberConfig::default();
        let pk = R1Pair::new(3).pk;
        let normal = r1_member_hash(&config, pk, false).expect("normal");
        let fast_forward = r1_member_hash(&config, pk, true).expect("ff");
        assert_ne!(normal, fast_forward);
    }
}
