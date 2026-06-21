use crate::config::SignerConfig;
use crate::error::SignerResult;
use crate::vault::context::{
    compute_vault_context_from_hashes, compute_vault_hashes, VaultContext, VaultCustodySnapshot,
};
use crate::vault::spend::{build_vault_spend_context_from_hashes, VaultSpendContext};

#[derive(Debug, Clone)]
pub struct VaultSession {
    pub display: VaultContext,
    pub spend: VaultSpendContext,
}

/// Resolve vault session.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn resolve_vault_session(config: SignerConfig) -> SignerResult<VaultSession> {
    let kms_public_key_hex = match config.kms_public_key_hex.clone() {
        Some(value) => value,
        None => {
            crate::kms::get_public_key_compressed_hex(
                &config.kms_runtime,
                &config.kms_key_id,
                &config.kms_region,
            )
            .await?
        }
    };

    build_vault_session(&config.vault, &kms_public_key_hex, &config)
}

/// Build vault session.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn build_vault_session(
    snapshot: &VaultCustodySnapshot,
    kms_public_key_hex: &str,
    config: &SignerConfig,
) -> SignerResult<VaultSession> {
    let hashes = compute_vault_hashes(snapshot)?;
    let display =
        compute_vault_context_from_hashes(snapshot, &hashes, kms_public_key_hex, &config.network)?;
    let spend = build_vault_spend_context_from_hashes(snapshot, &hashes, &display, config)?;
    Ok(VaultSession { display, spend })
}

/// Resolve vault spend context.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn resolve_vault_spend_context(config: SignerConfig) -> SignerResult<VaultSpendContext> {
    Ok(resolve_vault_session(config).await?.spend)
}

#[cfg(test)]
mod tests {
    use chia_protocol::Bytes32;

    use crate::config::load_program_bundle;
    use crate::kms::{KmsOverrides, KmsRuntime};
    use crate::test_support::minimal_program::{
        write_minimal_program_with_signer, MinimalProgramParams,
    };

    use super::{build_vault_session, resolve_vault_session};

    fn custody_kms_hex(config: &crate::config::SignerConfig) -> String {
        config.vault.custody_keys[0].public_key_hex.clone()
    }

    fn write_vault_session_program(path: &std::path::Path, home_dir: &std::path::Path) {
        write_minimal_program_with_signer(
            path,
            MinimalProgramParams {
                home_dir,
                ..Default::default()
            },
        );
        let contents = std::fs::read_to_string(path).expect("read program");
        std::fs::write(
            path,
            contents.replace(
                "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb9901baa6b7a99d\"\n      curve: SECP256R1",
                "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58\"\n      curve: BLS12_381",
            ),
        )
        .expect("write fixed program");
    }

    #[tokio::test]
    async fn resolve_vault_session_fetches_kms_public_key_when_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program_path = dir.path().join("program.yaml");
        write_vault_session_program(&program_path, dir.path());
        let mut config = load_program_bundle(&program_path)
            .expect("program bundle")
            .signer;
        let kms_hex = custody_kms_hex(&config);
        config.kms_public_key_hex = None;
        config.kms_runtime = KmsRuntime::test(KmsOverrides {
            public_key_compressed_hex: Some(kms_hex.clone()),
            fast_fail: false,
        });

        let session = resolve_vault_session(config)
            .await
            .expect("resolve vault session");
        assert_eq!(
            session.display.kms_public_key_hex,
            crate::kms::normalize_hex(&kms_hex)
        );
    }

    #[test]
    fn build_vault_session_uses_preloaded_kms_hex() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program_path = dir.path().join("program.yaml");
        write_vault_session_program(&program_path, dir.path());
        let config = load_program_bundle(&program_path)
            .expect("program bundle")
            .signer;
        let kms_hex = custody_kms_hex(&config);
        let session = build_vault_session(&config.vault, &kms_hex, &config).expect("session");
        assert_eq!(
            session.display.kms_public_key_hex,
            crate::kms::normalize_hex(&kms_hex)
        );
        assert_ne!(session.spend.launcher_id, Bytes32::default());
    }
}
