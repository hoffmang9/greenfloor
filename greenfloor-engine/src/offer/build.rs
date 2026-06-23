use crate::async_boundary::BuildVaultCatOfferFuture;
use crate::bech32m::decode_address;
use crate::coinset::{self, LiveCoinset, OfferCoinsetBackend};
use crate::config::SignerConfig;
use crate::error::SignerResult;
use crate::hex::hex_to_bytes32;
use crate::offer::assemble::{
    execute_direct_offer, execute_existing_presplit_offer, execute_presplit_new_offer,
};
use crate::offer::plan::{plan_vault_cat_offer, validate_offer_input, OfferPlan};
use crate::offer::types::{CreateOfferRequest, CreateOfferResult, OfferInput};
use crate::vault::session::resolve_vault_session;
use crate::vault::spend::VaultSpendContext;

/// Build vault cat offer.
///
/// # Errors
///
/// Returns an error if the operation fails.
#[must_use]
pub fn build_vault_cat_offer(
    config: SignerConfig,
    request: CreateOfferRequest,
) -> BuildVaultCatOfferFuture {
    Box::pin(build_vault_cat_offer_async(config, request))
}

async fn build_vault_cat_offer_async(
    config: SignerConfig,
    request: CreateOfferRequest,
) -> SignerResult<CreateOfferResult> {
    let input = OfferInput::try_from(request)?;
    validate_offer_input(&input)?;

    let coinset = coinset::client_for_config(&config)?;
    let mut session = resolve_vault_session(config).await?;
    let backend = LiveCoinset(&coinset);
    build_vault_cat_offer_with_spend(&mut session.spend, &backend, input).await
}

pub(crate) async fn build_vault_cat_offer_with_spend<C: OfferCoinsetBackend>(
    vault_ctx: &mut VaultSpendContext,
    backend: &C,
    input: OfferInput,
) -> SignerResult<CreateOfferResult> {
    let receive_puzzle_hash = decode_address(input.terms().receive_address.as_str())?;
    let offer_asset_id = hex_to_bytes32(&input.terms().offer_asset_id)?;

    match plan_vault_cat_offer(backend, &input, offer_asset_id).await? {
        OfferPlan::ExistingPresplit { offer_nonce } => {
            execute_existing_presplit_offer(
                backend,
                &input,
                receive_puzzle_hash,
                offer_asset_id,
                vault_ctx,
                offer_nonce,
            )
            .await
        }
        OfferPlan::RequiresSplitFlag => Err(crate::error::SignerError::OfferInputRequiresPresplit),
        OfferPlan::SplitAndOffer {
            selection,
            offer_nonce,
        } => {
            execute_presplit_new_offer(
                vault_ctx,
                backend,
                input,
                receive_puzzle_hash,
                selection,
                offer_nonce,
            )
            .await
        }
        OfferPlan::Direct {
            selection,
            offer_nonce,
        } => {
            execute_direct_offer(
                vault_ctx,
                backend,
                input,
                receive_puzzle_hash,
                offer_asset_id,
                selection,
                offer_nonce,
            )
            .await
        }
    }
}
