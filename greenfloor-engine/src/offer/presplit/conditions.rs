use chia_protocol::Bytes32;
use chia_puzzles::SETTLEMENT_PAYMENT_HASH;
use chia_sdk_driver::{InnerPuzzleSpend, MipsSpend, Spend, SpendContext};
use chia_sdk_types::puzzles::{SingletonMember, SingletonMemberSolution};
use clvm_utils::TreeHash;
use clvmr::NodePtr;

use crate::error::{SignerError, SignerResult};
use crate::offer::plan::{build_offer_request_conditions, OfferPaymentBundle};
use crate::vault::members::p2_conditions_or_singleton_puzzle_hash;
use crate::vault::spend::VaultSpendContext;

fn insert_p2_conditions_m_of_n(
    mips_spend: &mut MipsSpend,
    fixed_conditions_hash: TreeHash,
    p2_singleton_hash: TreeHash,
    full_puzzle_hash: TreeHash,
) {
    mips_spend.members.insert(
        full_puzzle_hash,
        InnerPuzzleSpend::m_of_n(
            0,
            Vec::new(),
            1,
            vec![fixed_conditions_hash, p2_singleton_hash],
        ),
    );
}

pub(crate) fn build_fixed_presplit_conditions_spend(
    ctx: &mut SpendContext,
    payments: &OfferPaymentBundle,
    offer_amount: u64,
    expires_at: Option<u64>,
) -> SignerResult<Spend> {
    let mut conditions = build_offer_request_conditions(ctx, payments, expires_at)?;
    let settlement_memos = ctx
        .hint(SETTLEMENT_PAYMENT_HASH.into())
        .map_err(SignerError::from)?;
    conditions = conditions.create_coin(
        SETTLEMENT_PAYMENT_HASH.into(),
        offer_amount,
        settlement_memos,
    );
    ctx.delegated_spend(conditions).map_err(SignerError::from)
}

/// Build presplit conditions inner spend.
///
/// # Errors
///
/// Returns an error if MIPS spend construction fails.
pub fn build_presplit_conditions_inner_spend(
    ctx: &mut SpendContext,
    fixed_spend: Spend,
    launcher_id: Bytes32,
) -> SignerResult<Spend> {
    let hashes =
        p2_conditions_or_singleton_puzzle_hash(ctx.tree_hash(fixed_spend.puzzle), launcher_id)?;

    let mut mips_spend = MipsSpend::new(Spend::new(NodePtr::NIL, NodePtr::NIL));
    mips_spend.members.insert(
        hashes.fixed_conditions_hash,
        InnerPuzzleSpend::new(0, Vec::new(), fixed_spend),
    );
    insert_p2_conditions_m_of_n(
        &mut mips_spend,
        hashes.fixed_conditions_hash,
        hashes.p2_singleton_hash,
        hashes.puzzle_hash,
    );
    mips_spend
        .spend(ctx, hashes.puzzle_hash)
        .map_err(SignerError::from)
}

pub(crate) fn build_presplit_offer_cancel_inner_spend(
    ctx: &mut SpendContext,
    cancel_delegated: Spend,
    vault_ctx: &VaultSpendContext,
    fixed_delegated_puzzle_hash: TreeHash,
) -> SignerResult<Spend> {
    let hashes =
        p2_conditions_or_singleton_puzzle_hash(fixed_delegated_puzzle_hash, vault_ctx.launcher_id)?;

    let mut mips_spend = MipsSpend::new(cancel_delegated);
    let member = SingletonMember::new(vault_ctx.launcher_id);
    let member_puzzle = ctx.curry(member).map_err(SignerError::from)?;
    let member_solution = ctx
        .alloc(&SingletonMemberSolution::new(
            vault_ctx.inner_puzzle_hash.into(),
            1,
        ))
        .map_err(SignerError::from)?;
    mips_spend.members.insert(
        hashes.p2_singleton_hash,
        InnerPuzzleSpend::new(0, Vec::new(), Spend::new(member_puzzle, member_solution)),
    );
    insert_p2_conditions_m_of_n(
        &mut mips_spend,
        hashes.fixed_conditions_hash,
        hashes.p2_singleton_hash,
        hashes.puzzle_hash,
    );
    mips_spend
        .spend(ctx, hashes.puzzle_hash)
        .map_err(SignerError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_puzzle_types::Memos;
    use chia_sdk_driver::{AssetInfo, RequestedPayments};
    use chia_sdk_types::run_puzzle;
    use chia_sdk_types::Condition;
    use chia_sdk_types::Conditions;
    use clvm_traits::FromClvm;

    #[test]
    fn p2_conditions_or_singleton_hash_is_deterministic() {
        let launcher_id = Bytes32::new([0xcc; 32]);
        let mut ctx = SpendContext::new();
        let fixed_spend = ctx
            .delegated_spend(Conditions::new().create_coin(
                Bytes32::new([0xab; 32]),
                1,
                Memos::None,
            ))
            .expect("fixed spend");
        let puzzle_hash = ctx.tree_hash(fixed_spend.puzzle);
        let hashes =
            p2_conditions_or_singleton_puzzle_hash(puzzle_hash, launcher_id).expect("p2 hashes");
        assert_ne!(hashes.puzzle_hash.to_bytes(), [0u8; 32]);
        assert_ne!(hashes.fixed_conditions_hash.to_bytes(), [0u8; 32]);
    }

    #[test]
    fn fixed_presplit_conditions_include_settlement_output() {
        let mut ctx = SpendContext::new();
        let payments = OfferPaymentBundle {
            requested_payments: RequestedPayments::new(),
            requested_asset_info: AssetInfo::new(),
        };
        let fixed_spend = build_fixed_presplit_conditions_spend(&mut ctx, &payments, 1000, None)
            .expect("fixed spend");
        let output =
            run_puzzle(&mut ctx, fixed_spend.puzzle, fixed_spend.solution).expect("run puzzle");
        let conditions = Conditions::<NodePtr>::from_clvm(&ctx, output).expect("conditions");
        assert!(
            conditions
                .iter()
                .any(|condition| matches!(condition, Condition::CreateCoin(create)
                    if create.puzzle_hash == SETTLEMENT_PAYMENT_HASH.into() && create.amount == 1000))
        );
    }
}
