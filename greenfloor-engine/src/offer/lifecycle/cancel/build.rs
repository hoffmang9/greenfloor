use crate::adapters::DexieClient;
use crate::coinset::{client_for_signer_on_network, spend_bundle_operation_id, LiveCoinset};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::offer::cancel_input::metadata_sufficient_for_coinset_cancel;
use crate::offer::lifecycle::reconcile_prep::fetch_dexie_offer_file_text;
use crate::offer::reclaim::{
    build_offer_cancel_spend_bundle, build_offer_cancel_spend_bundle_from_metadata,
};
use crate::offer::types::StoredOfferCancelMetadata;
use crate::vault::session::resolve_vault_spend_context;
use chia_protocol::SpendBundle;

#[derive(Debug)]
enum CancelInput<'a> {
    OfferFile(String),
    StoredMetadata(&'a StoredOfferCancelMetadata),
}

/// Whether cancel still needs a Dexie offer-file fetch (ADR 0015 input order).
#[must_use]
pub(super) fn needs_dexie_offer_file(
    local_text: Option<&str>,
    cancel_metadata: Option<&StoredOfferCancelMetadata>,
) -> bool {
    local_text.is_none() && !metadata_sufficient_for_coinset_cancel(cancel_metadata)
}

fn missing_cancel_input_error() -> SignerError {
    SignerError::Other(
        "offer cancel requires local offer file, stored cancel metadata, or Dexie offer-file fallback"
            .to_string(),
    )
}

/// Local text → metadata-sufficient (no blob) → optional Dexie offer-file fallback.
async fn resolve_cancel_input<'a>(
    offer_id: &str,
    local_text: Option<&str>,
    cancel_metadata: Option<&'a StoredOfferCancelMetadata>,
    dexie: Option<&DexieClient>,
) -> SignerResult<CancelInput<'a>> {
    if let Some(text) = local_text {
        return Ok(CancelInput::OfferFile(text.to_string()));
    }
    if let Some(metadata) =
        cancel_metadata.filter(|meta| metadata_sufficient_for_coinset_cancel(Some(meta)))
    {
        return Ok(CancelInput::StoredMetadata(metadata));
    }
    match dexie {
        Some(client) => Ok(CancelInput::OfferFile(
            fetch_dexie_offer_file_text(client, offer_id).await?,
        )),
        None => Err(missing_cancel_input_error()),
    }
}

/// Resolve offer inputs before vault/KMS so Dexie/local failures surface without
/// requiring signer credentials.
pub(super) async fn build_cancel_spend_bundle(
    signer_config: &SignerConfig,
    operator_network: &str,
    offer_id: &str,
    local_text: Option<&str>,
    cancel_metadata: Option<&StoredOfferCancelMetadata>,
    dexie: Option<&DexieClient>,
) -> SignerResult<(SpendBundle, String, chia_sdk_coinset::CoinsetClient)> {
    let input = resolve_cancel_input(offer_id, local_text, cancel_metadata, dexie).await?;
    let coinset_client = client_for_signer_on_network(signer_config, operator_network)?;
    let backend = LiveCoinset(&coinset_client);
    let mut vault_ctx = resolve_vault_spend_context(signer_config.clone()).await?;

    let spend_bundle = match input {
        CancelInput::OfferFile(text) => {
            build_offer_cancel_spend_bundle(&mut vault_ctx, &backend, &text, cancel_metadata)
                .await?
        }
        CancelInput::StoredMetadata(metadata) => {
            build_offer_cancel_spend_bundle_from_metadata(&mut vault_ctx, &backend, metadata)
                .await?
        }
    };
    let operation_id = spend_bundle_operation_id(&spend_bundle)?;
    Ok((spend_bundle, operation_id, coinset_client))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::types::{OfferCancelFields, OfferExecutionMode};

    fn sufficient_metadata() -> StoredOfferCancelMetadata {
        StoredOfferCancelMetadata {
            fields: OfferCancelFields::from_direct_build("11".repeat(32), "22".repeat(32)),
            execution_mode: Some(OfferExecutionMode::Direct),
        }
    }

    #[test]
    fn needs_dexie_false_with_local_text_or_sufficient_metadata() {
        let meta = sufficient_metadata();
        assert!(!needs_dexie_offer_file(Some("offer1abc"), None));
        assert!(!needs_dexie_offer_file(None, Some(&meta)));
        assert!(needs_dexie_offer_file(None, None));
    }

    #[tokio::test]
    async fn resolve_prefers_local_text_then_metadata() {
        let meta = sufficient_metadata();
        match resolve_cancel_input("offer-1", Some("offer1local"), Some(&meta), None)
            .await
            .expect("resolve")
        {
            CancelInput::OfferFile(text) => assert_eq!(text, "offer1local"),
            CancelInput::StoredMetadata(_) => panic!("expected offer file"),
        }
        match resolve_cancel_input("offer-1", None, Some(&meta), None)
            .await
            .expect("resolve")
        {
            CancelInput::StoredMetadata(stored) => {
                assert_eq!(
                    stored.fields.input_coin_id.as_deref(),
                    Some(&*"11".repeat(32))
                );
            }
            CancelInput::OfferFile(_) => panic!("expected metadata"),
        }
    }

    #[tokio::test]
    async fn resolve_errors_when_dexie_fallback_unavailable() {
        let err = resolve_cancel_input("offer-1", None, None, None)
            .await
            .expect_err("missing inputs");
        assert!(err.to_string().contains("offer cancel requires"));
    }
}
