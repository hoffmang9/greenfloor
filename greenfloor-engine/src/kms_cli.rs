use clap::Args;

use crate::cli_util::print_json_value;
use crate::error::SignerResult;
use crate::kms::{self, KmsRuntime};

#[derive(Debug, Args)]
pub struct KmsPublicKeyArgs {
    #[arg(long)]
    pub key_id: String,
    #[arg(long)]
    pub region: String,
    #[arg(long)]
    pub json: bool,
}

/// Run kms public key compressed hex.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_kms_public_key_compressed_hex(args: KmsPublicKeyArgs) -> SignerResult<()> {
    run_kms_public_key_compressed_hex_with_runtime(args, &KmsRuntime::production()).await
}

/// Run kms public key compressed hex with an explicit KMS runtime boundary.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_kms_public_key_compressed_hex_with_runtime(
    args: KmsPublicKeyArgs,
    runtime: &KmsRuntime,
) -> SignerResult<()> {
    let compressed_hex =
        kms::get_public_key_compressed_hex(runtime, &args.key_id, &args.region).await?;
    if args.json {
        print_json_value(
            &serde_json::json!({ "public_key_compressed_hex": compressed_hex }),
            true,
        )?;
    } else {
        println!("{compressed_hex}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SignerError;
    use crate::kms::KmsOverrides;
    use serde_json::{json, Value};

    #[tokio::test]
    async fn kms_public_key_emits_json_shape_in_process() {
        let runtime = KmsRuntime::test(KmsOverrides {
            public_key_compressed_hex: Some("02abc123".to_string()),
            fast_fail: false,
        });
        let hex = kms::get_public_key_compressed_hex(
            &runtime,
            "arn:aws:kms:us-east-1:123456789012:key/demo",
            "us-east-1",
        )
        .await
        .expect("stubbed kms public key");
        let payload = json!({ "public_key_compressed_hex": hex });
        assert_eq!(
            payload
                .get("public_key_compressed_hex")
                .and_then(Value::as_str),
            Some("02abc123")
        );
    }

    #[tokio::test]
    async fn kms_public_key_fast_fail_reports_credentials_error() {
        let runtime = KmsRuntime::test(KmsOverrides {
            public_key_compressed_hex: None,
            fast_fail: true,
        });
        let err = kms::get_public_key_compressed_hex(
            &runtime,
            "arn:aws:kms:us-east-1:123456789012:key/demo",
            "us-east-1",
        )
        .await
        .expect_err("fast fail kms");
        assert!(matches!(err, SignerError::Kms(_)));
        assert!(
            err.to_string().to_ascii_lowercase().contains("credentials"),
            "unexpected kms failure: {err}"
        );
    }

    #[tokio::test]
    async fn run_kms_public_key_command_uses_stubbed_hex() {
        let runtime = KmsRuntime::test(KmsOverrides {
            public_key_compressed_hex: Some("02deadbeef".to_string()),
            fast_fail: false,
        });
        run_kms_public_key_compressed_hex_with_runtime(
            KmsPublicKeyArgs {
                key_id: "arn:aws:kms:us-east-1:123456789012:key/demo".to_string(),
                region: "us-east-1".to_string(),
                json: true,
            },
            &runtime,
        )
        .await
        .expect("kms command");
    }
}
