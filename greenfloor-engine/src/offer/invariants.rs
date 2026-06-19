use chia_protocol::SpendBundle;
use chia_sdk_types::{run_puzzle, Condition, Conditions};
use clvm_traits::FromClvm;
use clvmr::{serde::node_from_bytes, Allocator, NodePtr};

use crate::error::{SignerError, SignerResult};
use crate::offer::types::OfferExecutionMode;

/// Presplit offer bundles must not include vault singleton spends that block mempool fast-forward.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn assert_presplit_offer_fast_forward_eligible(
    spend_bundle: &SpendBundle,
    execution_mode: OfferExecutionMode,
) -> SignerResult<()> {
    if !matches!(
        execution_mode,
        OfferExecutionMode::PresplitNew | OfferExecutionMode::PresplitExisting
    ) {
        return Ok(());
    }
    for coin_spend in &spend_bundle.coin_spends {
        let mut allocator = Allocator::new();
        let puzzle = node_from_bytes(&mut allocator, coin_spend.puzzle_reveal.as_ref())
            .map_err(|err| SignerError::Driver(err.to_string()))?;
        let solution = node_from_bytes(&mut allocator, coin_spend.solution.as_ref())
            .map_err(|err| SignerError::Driver(err.to_string()))?;
        let output = run_puzzle(&mut allocator, puzzle, solution)
            .map_err(|err| SignerError::Driver(err.to_string()))?;
        let conditions = Conditions::<NodePtr>::from_clvm(&allocator, output)
            .map_err(|err| SignerError::Driver(err.to_string()))?;
        for condition in conditions.iter() {
            match condition {
                Condition::AggSigMe(_)
                | Condition::AggSigUnsafe(_)
                | Condition::AggSigParent(_)
                | Condition::AggSigPuzzle(_)
                | Condition::AggSigParentPuzzle(_)
                | Condition::AssertMyCoinId(_) => {
                    return Err(SignerError::Other(
                        "presplit offer bundle is not fast-forward eligible".to_string(),
                    ));
                }
                Condition::ReceiveMessage(receive) if receive.mode == 23 => {
                    return Err(SignerError::Other(
                        "presplit offer bundle emits mode-23 receive message".to_string(),
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_protocol::SpendBundle;

    #[test]
    fn presplit_eligibility_skips_direct_mode() {
        let bundle = SpendBundle::new(vec![], chia_bls::Signature::default());
        assert_presplit_offer_fast_forward_eligible(&bundle, OfferExecutionMode::Direct).unwrap();
    }

    #[test]
    fn empty_presplit_bundle_passes() {
        let bundle = SpendBundle::new(vec![], chia_bls::Signature::default());
        assert_presplit_offer_fast_forward_eligible(&bundle, OfferExecutionMode::PresplitNew)
            .unwrap();
    }
}
