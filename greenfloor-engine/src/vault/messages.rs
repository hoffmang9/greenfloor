use chia_protocol::Bytes32;
use chia_sdk_driver::SpendContext;
use chia_sdk_types::{run_puzzle, Condition, Conditions};
use clvm_traits::FromClvm;
use clvmr::{serde::node_from_bytes, Allocator, NodePtr};

use crate::error::{SignerError, SignerResult};

/// Extract mode23 receive messages.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn extract_mode23_receive_messages(
    ctx: &SpendContext,
) -> SignerResult<Vec<(Vec<u8>, Bytes32)>> {
    let mut messages = Vec::new();
    for coin_spend in ctx.iter() {
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
            if let Condition::ReceiveMessage(receive) = condition {
                if receive.mode == 23 {
                    messages.push((receive.message.to_vec(), coin_spend.coin.coin_id()));
                }
            }
        }
    }
    Ok(messages)
}
