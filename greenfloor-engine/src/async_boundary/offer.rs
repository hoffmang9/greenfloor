use std::future::Future;
use std::pin::Pin;

use crate::error::SignerResult;
use crate::offer::operator::BuildAndPostOfferResponse;
use crate::offer::types::CreateOfferResult;

/// Boxed future for managed offer build/post (daemon + CLI).
pub type BuildAndPostOfferFuture =
    Pin<Box<dyn Future<Output = SignerResult<BuildAndPostOfferResponse>> + Send>>;

/// Boxed future for vault CAT offer construction (`greenfloor-engine create-offer`).
pub type BuildVaultCatOfferFuture =
    Pin<Box<dyn Future<Output = SignerResult<CreateOfferResult>> + Send>>;
