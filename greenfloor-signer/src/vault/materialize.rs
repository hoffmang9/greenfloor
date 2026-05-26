use chia_protocol::SpendBundle;
use chia_puzzle_types::Memos;
use chia_sdk_driver::{
    mips_puzzle_hash, Cat, CatSpend, InnerPuzzleSpend, MipsSpend, Spend, SpendContext, Vault,
};
use chia_sdk_types::{
    conditions::SendMessage,
    puzzles::{
        R1MemberPuzzleAssert, R1MemberPuzzleAssertSolution, SingletonMember,
        SingletonMemberSolution,
    },
    Conditions, Mod,
};
use chia_secp::R1Signature;
use clvm_utils::TreeHash;

use crate::coinset::OfferCoinsetBackend;
use crate::error::{SignerError, SignerResult};
use crate::vault::messages::extract_mode23_receive_messages;
use crate::vault::spend::{VaultFastForwardSigner, VaultSpendContext};

pub(crate) async fn materialize_vault_cat_finished_spends<C: OfferCoinsetBackend>(
    ctx: &mut SpendContext,
    vault_ctx: &mut VaultSpendContext,
    coinset: &C,
    finished: chia_sdk_driver::Spends<chia_sdk_driver::Finished>,
) -> SignerResult<SpendBundle> {
    let vault = coinset
        .fetch_latest_vault(vault_ctx.launcher_id, vault_ctx.inner_puzzle_hash)
        .await?;
    let signer = VaultFastForwardSigner::from_context(vault_ctx);
    materialize_vault_cat_finished_spends_with_vault(
        ctx,
        vault_ctx,
        finished,
        vault,
        move |message| {
            let signer = signer.clone();
            async move { signer.sign(message).await }
        },
    )
    .await
}

pub(crate) async fn materialize_vault_cat_finished_spends_with_vault<F, Fut>(
    ctx: &mut SpendContext,
    vault_ctx: &mut VaultSpendContext,
    finished: chia_sdk_driver::Spends<chia_sdk_driver::Finished>,
    vault: Vault,
    sign_digest: F,
) -> SignerResult<SpendBundle>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = SignerResult<R1Signature>>,
{
    let mut cat_spends = Vec::new();
    for (asset, kind) in finished.unspent() {
        let chia_sdk_driver::SpendableAsset::Cat(cat) = asset else {
            continue;
        };
        let chia_sdk_driver::SpendKind::Conditions(spend) = kind else {
            return Err(SignerError::Driver(
                "unexpected settlement spend in vault cat spend".to_string(),
            ));
        };
        let delegated = ctx
            .delegated_spend(spend.finish())
            .map_err(SignerError::from)?;
        let nonce = vault_ctx
            .infer_nonce_for_p2_hash(cat.info.p2_puzzle_hash)
            .ok_or(SignerError::Driver(
                "failed to infer vault nonce for cat p2 puzzle hash".to_string(),
            ))?;
        let inner_spend = build_vault_cat_inner_spend(
            ctx,
            delegated,
            vault_ctx,
            nonce,
            cat.info.p2_puzzle_hash.into(),
        )?;
        cat_spends.push(CatSpend::new(cat, inner_spend));
    }
    if cat_spends.is_empty() {
        return Err(SignerError::Driver(
            "no cat spends produced for vault transaction".to_string(),
        ));
    }
    Cat::spend_all(ctx, &cat_spends).map_err(SignerError::from)?;
    append_vault_singleton_spend_for_vault(ctx, vault_ctx, &vault, sign_digest).await?;
    Ok(SpendBundle::new(ctx.take(), chia_bls::Signature::default()))
}

pub(crate) fn build_vault_cat_inner_spend(
    ctx: &mut SpendContext,
    delegated: Spend,
    vault_ctx: &VaultSpendContext,
    nonce: u32,
    p2_puzzle_hash: TreeHash,
) -> SignerResult<Spend> {
    let mut mips_spend = MipsSpend::new(delegated);
    let restrictions = Vec::new();
    let member = SingletonMember::new(vault_ctx.launcher_id);
    let member_hash = mips_puzzle_hash(
        nonce as usize,
        restrictions.clone(),
        member.curry_tree_hash(),
        true,
    );
    let member_puzzle = ctx.curry(member).map_err(SignerError::from)?;
    let member_solution = ctx
        .alloc(&SingletonMemberSolution::new(
            vault_ctx.inner_puzzle_hash.into(),
            1,
        ))
        .map_err(SignerError::from)?;
    mips_spend.members.insert(
        member_hash,
        InnerPuzzleSpend::new(
            nonce as usize,
            restrictions,
            Spend::new(member_puzzle, member_solution),
        ),
    );
    mips_spend
        .spend(ctx, p2_puzzle_hash)
        .map_err(SignerError::from)
}

pub(crate) async fn append_vault_singleton_spend_for_vault<F, Fut>(
    ctx: &mut SpendContext,
    vault_ctx: &VaultSpendContext,
    vault: &Vault,
    sign_digest: F,
) -> SignerResult<()>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = SignerResult<R1Signature>>,
{
    let receive_messages = extract_mode23_receive_messages(ctx)?;
    if receive_messages.is_empty() {
        return Err(SignerError::VaultReceiveMessageNotFound);
    }
    let mut conditions = Conditions::new().create_coin(
        vault_ctx.inner_puzzle_hash.into(),
        vault.coin.amount,
        Memos::None,
    );
    for (message, coin_id) in receive_messages {
        let coin_ptr = ctx.alloc(&coin_id).map_err(SignerError::from)?;
        conditions = conditions.with(SendMessage::new(23, message.into(), vec![coin_ptr]));
    }
    let delegated_spend = ctx.delegated_spend(conditions).map_err(SignerError::from)?;
    let delegated_hash = ctx.tree_hash(delegated_spend.puzzle);
    let signature_message = [delegated_hash.to_bytes(), vault.coin.puzzle_hash.to_bytes()].concat();
    let signature = sign_digest(signature_message).await?;

    let mut mips_spend = MipsSpend::new(delegated_spend);
    mips_spend.members.insert(
        vault_ctx.inner_puzzle_hash,
        InnerPuzzleSpend::m_of_n(
            0,
            Vec::new(),
            1,
            vec![vault_ctx.custody_hash, vault_ctx.recovery_hash],
        ),
    );

    let member = R1MemberPuzzleAssert::new(vault_ctx.secp256r1_public_key);
    let member_puzzle = ctx.curry(member).map_err(SignerError::from)?;
    let member_solution = ctx
        .alloc(&R1MemberPuzzleAssertSolution::new(
            vault.coin.puzzle_hash,
            signature,
        ))
        .map_err(SignerError::from)?;
    mips_spend.members.insert(
        vault_ctx.custody_hash,
        InnerPuzzleSpend::new(0, Vec::new(), Spend::new(member_puzzle, member_solution)),
    );

    vault.spend(ctx, &mips_spend).map_err(SignerError::from)?;
    Ok(())
}
