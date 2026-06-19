use std::future::Future;
use std::pin::Pin;

use crate::error::SignerResult;

use super::types::BootstrapPhaseResult;

/// Boxed future for the signer denomination bootstrap phase.
///
/// Boxed here because the async state machine exceeds Clippy's large-futures threshold
/// once ladder planning and vault split submission are composed.
pub(super) type SignerDenominationPhaseFuture<'a> =
    Pin<Box<dyn Future<Output = SignerResult<BootstrapPhaseResult>> + Send + 'a>>;
