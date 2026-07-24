use crate::adapters::DexieClient;
use crate::coinset::{client_for_signer_on_network, spend_bundle_operation_id, LiveCoinset};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::offer::cancel_input::metadata_sufficient_for_coinset_cancel;
use crate::offer::dexie_payload::DexieOfferPayload;
use crate::offer::reclaim::{
    build_offer_cancel_spend_bundle, build_offer_cancel_spend_bundle_from_metadata,
};
use crate::offer::types::StoredOfferCancelMetadata;
use crate::vault::session::resolve_vault_spend_context;
use chia_protocol::SpendBundle;

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

async fn fetch_dexie_offer_file_text(dexie: &DexieClient, offer_id: &str) -> SignerResult<String> {
    let response = dexie.get_offer(offer_id).await?;
    if response.is_explicit_failure() {
        return Err(SignerError::OfferCancelOfferFileNotFound);
    }
    let payload = DexieOfferPayload::new(response.into_value());
    payload
        .offer_file_text()
        .map(str::to_string)
        .ok_or(SignerError::OfferCancelOfferFileMissing)
}

/// Local text → metadata-sufficient (no blob) → optional Dexie offer-file fallback.
async fn resolve_cancel_input<'a>(
    offer_id: &str,
    local_text: Option<&str>,
    cancel_metadata: Option<&'a StoredOfferCancelMetadata>,
    dexie: Option<&DexieClient>,
) -> SignerResult<CancelInput<'a>> {
    if !needs_dexie_offer_file(local_text, cancel_metadata) {
        if let Some(text) = local_text {
            return Ok(CancelInput::OfferFile(text.to_string()));
        }
        // `needs_dexie` is false without local text ⇒ metadata is sufficient.
        let metadata = cancel_metadata.ok_or_else(missing_cancel_input_error)?;
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
