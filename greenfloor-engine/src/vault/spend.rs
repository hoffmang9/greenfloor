use std::collections::HashMap;
#[cfg(test)]
use std::sync::Arc;

use chia_protocol::Bytes32;
use chia_secp::{R1PublicKey, R1Signature};
use clvm_utils::TreeHash;

use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::hex_to_bytes;
use crate::kms::{self, KmsRuntime};
use crate::vault::context::{VaultComputedHashes, VaultContext, VaultCustodySnapshot};
use crate::vault::members::nonce_member_puzzle_hash;

#[derive(Debug, Clone)]
pub struct KmsSigner {
    key_id: String,
    region: String,
    runtime: KmsRuntime,
}

impl KmsSigner {
    #[must_use]
    pub fn from_vault_context(vault_ctx: &VaultSpendContext) -> Self {
        Self {
            key_id: vault_ctx.kms_key_id.clone(),
            region: vault_ctx.kms_region.clone(),
            runtime: vault_ctx.kms_runtime.clone(),
        }
    }

    /// Sign fast forward digest.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn sign_fast_forward_digest(
        &self,
        signature_message: Vec<u8>,
    ) -> SignerResult<R1Signature> {
        sign_vault_fast_forward_digest(&self.runtime, &self.key_id, &self.region, signature_message)
            .await
    }
}

#[cfg(test)]
pub(crate) type LocalFastForwardSigner =
    Arc<dyn Fn(Vec<u8>) -> SignerResult<R1Signature> + Send + Sync>;

#[derive(Clone)]
pub(crate) struct VaultFastForwardSigner {
    kms: KmsSigner,
    #[cfg(test)]
    local: Option<LocalFastForwardSigner>,
}

impl VaultFastForwardSigner {
    pub fn from_context(vault_ctx: &VaultSpendContext) -> Self {
        Self {
            kms: KmsSigner::from_vault_context(vault_ctx),
            #[cfg(test)]
            local: vault_ctx.local_fast_forward_signer.clone(),
        }
    }

    pub async fn sign(&self, message: Vec<u8>) -> SignerResult<R1Signature> {
        #[cfg(test)]
        if let Some(local) = &self.local {
            return local(message);
        }
        self.kms.sign_fast_forward_digest(message).await
    }
}

#[derive(Clone)]
pub struct VaultSpendContext {
    pub launcher_id: Bytes32,
    pub inner_puzzle_hash: TreeHash,
    pub custody_hash: TreeHash,
    pub recovery_hash: TreeHash,
    pub kms_key_id: String,
    pub kms_region: String,
    pub kms_runtime: KmsRuntime,
    pub secp256r1_public_key: R1PublicKey,
    pub max_nonce_probe: u32,
    pub network: String,
    nonce_by_p2_hash: HashMap<Bytes32, u32>,
    #[cfg(test)]
    pub(crate) local_fast_forward_signer: Option<LocalFastForwardSigner>,
}

impl VaultSpendContext {
    pub fn infer_nonce_for_p2_hash(&mut self, p2_puzzle_hash: Bytes32) -> Option<u32> {
        if let Some(cached) = self.nonce_by_p2_hash.get(&p2_puzzle_hash) {
            return Some(*cached);
        }
        for nonce in 0..=self.max_nonce_probe {
            let Ok(candidate) = nonce_member_puzzle_hash(self.launcher_id, nonce) else {
                continue;
            };
            if Bytes32::from(candidate) == p2_puzzle_hash {
                self.nonce_by_p2_hash.insert(p2_puzzle_hash, nonce);
                return Some(nonce);
            }
        }
        None
    }

    #[cfg(test)]
    pub fn seed_nonce_cache(&mut self, p2_puzzle_hash: Bytes32, nonce: u32) {
        self.nonce_by_p2_hash.insert(p2_puzzle_hash, nonce);
    }

    #[cfg(test)]
    pub fn set_local_fast_forward_signer(&mut self, signer: LocalFastForwardSigner) {
        self.local_fast_forward_signer = Some(signer);
    }

    #[cfg(test)]
    #[must_use]
    pub fn new_test_context(
        launcher_id: Bytes32,
        inner_puzzle_hash: TreeHash,
        custody_hash: TreeHash,
        recovery_hash: TreeHash,
        secp256r1_public_key: R1PublicKey,
    ) -> Self {
        Self {
            launcher_id,
            inner_puzzle_hash,
            custody_hash,
            recovery_hash,
            kms_key_id: "test-kms".to_string(),
            kms_region: "us-west-2".to_string(),
            kms_runtime: KmsRuntime::production(),
            secp256r1_public_key,
            max_nonce_probe: 2048,
            network: "mainnet".to_string(),
            nonce_by_p2_hash: HashMap::default(),
            #[cfg(test)]
            local_fast_forward_signer: None,
        }
    }
}

impl std::fmt::Debug for VaultSpendContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultSpendContext")
            .field("launcher_id", &self.launcher_id)
            .field("inner_puzzle_hash", &self.inner_puzzle_hash)
            .field("custody_hash", &self.custody_hash)
            .field("recovery_hash", &self.recovery_hash)
            .field("kms_key_id", &self.kms_key_id)
            .field("kms_region", &self.kms_region)
            .field("secp256r1_public_key", &self.secp256r1_public_key)
            .field("max_nonce_probe", &self.max_nonce_probe)
            .field("network", &self.network)
            .finish_non_exhaustive()
    }
}

/// Build vault spend context from hashes.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn build_vault_spend_context_from_hashes(
    snapshot: &VaultCustodySnapshot,
    hashes: &VaultComputedHashes,
    display: &VaultContext,
    config: &SignerConfig,
) -> SignerResult<VaultSpendContext> {
    let key_bytes = hex_to_bytes(&display.secp256r1_custody_keys[0])?;
    let mut key_array = [0u8; 33];
    key_array.copy_from_slice(&key_bytes);
    let secp256r1_public_key = R1PublicKey::from_bytes(&key_array)
        .map_err(|err| SignerError::UnsupportedVaultCurve(format!("SECP256R1 decode: {err}")))?;
    Ok(VaultSpendContext {
        launcher_id: snapshot.launcher_id,
        inner_puzzle_hash: hashes.inner_puzzle_hash,
        custody_hash: hashes.custody_hash,
        recovery_hash: hashes.recovery_hash,
        kms_key_id: config.kms_key_id.clone(),
        kms_region: config.kms_region.clone(),
        kms_runtime: config.kms_runtime.clone(),
        secp256r1_public_key,
        max_nonce_probe: 2048,
        network: config.network.clone(),
        nonce_by_p2_hash: HashMap::from([(hashes.p2_singleton_message_hash.into(), 0)]),
        #[cfg(test)]
        local_fast_forward_signer: None,
    })
}

pub(crate) async fn sign_vault_fast_forward_digest(
    runtime: &KmsRuntime,
    kms_key_id: &str,
    kms_region: &str,
    signature_message: Vec<u8>,
) -> SignerResult<R1Signature> {
    let signature_hex = kms::sign_digest(
        runtime,
        kms_key_id,
        kms_region,
        &hex::encode(signature_message),
    )
    .await?;
    let signature_bytes = hex::decode(crate::hex::normalize_hex(&signature_hex))
        .map_err(|err| SignerError::Kms(format!("invalid signature hex: {err}")))?;
    let signature_array: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| SignerError::Kms("invalid compact signature length".to_string()))?;
    R1Signature::from_bytes(&signature_array)
        .map_err(|err| SignerError::Kms(format!("invalid r1 signature: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_protocol::Bytes32;
    use chia_sdk_test::R1Pair;

    #[test]
    fn infer_vault_nonce_for_p2_hash_matches_nonzero_nonce() {
        let launcher_id = Bytes32::new([0x11; 32]);
        let r1 = R1Pair::new(99);
        let mut vault_ctx = VaultSpendContext {
            launcher_id,
            inner_puzzle_hash: clvm_utils::TreeHash::from(launcher_id),
            custody_hash: clvm_utils::TreeHash::from(Bytes32::new([0x22; 32])),
            recovery_hash: clvm_utils::TreeHash::from(Bytes32::new([0x33; 32])),
            kms_key_id: String::new(),
            kms_region: String::new(),
            kms_runtime: KmsRuntime::production(),
            secp256r1_public_key: r1.pk,
            max_nonce_probe: 20,
            network: "mainnet".to_string(),
            nonce_by_p2_hash: HashMap::default(),
            #[cfg(test)]
            local_fast_forward_signer: None,
        };
        let target = crate::vault::members::nonce_member_puzzle_hash(launcher_id, 7)
            .expect("singleton hash");
        let inferred = vault_ctx
            .infer_nonce_for_p2_hash(target.into())
            .expect("nonce");
        assert_eq!(inferred, 7);
    }

    #[test]
    fn infer_vault_nonce_uses_cache_without_rescanning() {
        let launcher_id = Bytes32::new([0x44; 32]);
        let r1 = R1Pair::new(1);
        let p2_hash = Bytes32::new([0x55; 32]);
        let mut vault_ctx = VaultSpendContext::new_test_context(
            launcher_id,
            clvm_utils::TreeHash::from(launcher_id),
            clvm_utils::TreeHash::from(Bytes32::new([0x66; 32])),
            clvm_utils::TreeHash::from(Bytes32::new([0x77; 32])),
            r1.pk,
        );
        vault_ctx.max_nonce_probe = 0;
        vault_ctx.seed_nonce_cache(p2_hash, 12);
        assert_eq!(vault_ctx.infer_nonce_for_p2_hash(p2_hash), Some(12));
    }

    #[test]
    fn build_vault_spend_context_from_hashes_seeds_nonce_zero() {
        use crate::hex::hex_to_bytes32;
        use crate::test_support::golden::{golden_snapshot, CUSTODY_KEY_HEX, LAUNCHER_ID_HEX};
        use crate::vault::context::{compute_vault_context_from_hashes, compute_vault_hashes};

        let snapshot = golden_snapshot();
        let hashes = compute_vault_hashes(&snapshot).expect("hashes");
        let display =
            compute_vault_context_from_hashes(&snapshot, &hashes, CUSTODY_KEY_HEX, "mainnet")
                .expect("display");
        let config = crate::test_support::signer_config::test_signer_config("");
        let mut spend =
            build_vault_spend_context_from_hashes(&snapshot, &hashes, &display, &config)
                .expect("spend context");
        assert_eq!(
            spend.launcher_id,
            hex_to_bytes32(LAUNCHER_ID_HEX).expect("launcher")
        );
        assert_eq!(spend.kms_key_id, "kms-test");
        assert_eq!(
            spend.infer_nonce_for_p2_hash(hashes.p2_singleton_message_hash.into()),
            Some(0)
        );
    }

    #[test]
    fn vault_fast_forward_signer_uses_local_signer_in_tests() {
        use sha2::{Digest, Sha256};

        let r1 = R1Pair::new(42);
        let sk = r1.sk.clone();
        let mut vault_ctx = VaultSpendContext::new_test_context(
            Bytes32::new([0x01; 32]),
            clvm_utils::TreeHash::from(Bytes32::new([0x02; 32])),
            clvm_utils::TreeHash::from(Bytes32::new([0x03; 32])),
            clvm_utils::TreeHash::from(Bytes32::new([0x04; 32])),
            r1.pk,
        );
        vault_ctx.set_local_fast_forward_signer(Arc::new(move |message| {
            let digest: [u8; 32] = Sha256::digest(&message).into();
            sk.sign_prehashed(&digest)
                .map_err(|err| SignerError::Kms(err.to_string()))
        }));
        let signer = VaultFastForwardSigner::from_context(&vault_ctx);
        let kms = KmsSigner::from_vault_context(&vault_ctx);
        assert_eq!(kms.key_id, "test-kms");
        assert_eq!(kms.region, "us-west-2");

        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let signature = rt
            .block_on(signer.sign(b"fast-forward-message".to_vec()))
            .expect("local signature");
        assert_eq!(signature.to_bytes().len(), 64);
    }
}
