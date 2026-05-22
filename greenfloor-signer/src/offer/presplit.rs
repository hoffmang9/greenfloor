use chia_protocol::{Bytes32, Coin, CoinSpend, SpendBundle};
use chia_sdk_coinset::ChiaRpcClient;
use chia_puzzle_types::Memos;
use chia_puzzles::SETTLEMENT_PAYMENT_HASH;
use chia_sdk_coinset::CoinsetClient;
use chia_sdk_driver::{
    AssetInfo, Cat, CatSpend, DriverError, InnerPuzzleSpend, MipsSpend, Offer, RequestedPayments,
    Spend, SpendContext, Vault, encode_offer,
};
use chia_sdk_types::{
    Conditions,
    conditions::AssertBeforeSecondsAbsolute,
};
use clvmr::{Allocator, NodePtr};

use crate::coinset;
use crate::error::{SignerError, SignerResult};
use crate::vault::members::{
    MemberConfig, custom_member_hash, m_of_n_hash, singleton_member_hash,
};
use crate::vault::spend::{VaultSpendContext, build_vault_cat_inner_spend};

pub fn should_presplit(offered_total: u64, offer_amount: u64, split_input_coins: bool) -> bool {
    split_input_coins && offered_total > offer_amount
}

pub fn build_fixed_presplit_conditions_spend(
    ctx: &mut SpendContext,
    requested_payments: &RequestedPayments,
    asset_info: &AssetInfo,
    offer_amount: u64,
    expires_at: Option<u64>,
) -> SignerResult<Spend> {
    let assertions = requested_payments
        .assertions(ctx, asset_info)
        .map_err(driver_err)?;
    let mut conditions = Conditions::new();
    for assertion in assertions {
        conditions = conditions.with(assertion);
    }
    if let Some(seconds) = expires_at {
        conditions = conditions.with(AssertBeforeSecondsAbsolute::new(seconds));
    }
    let settlement_memos = ctx
        .hint(SETTLEMENT_PAYMENT_HASH.into())
        .map_err(driver_err)?;
    conditions = conditions.create_coin(
        SETTLEMENT_PAYMENT_HASH.into(),
        offer_amount,
        settlement_memos,
    );
    ctx.delegated_spend(conditions).map_err(driver_err)
}

pub fn build_presplit_conditions_inner_spend(
    ctx: &mut SpendContext,
    fixed_spend: Spend,
    launcher_id: Bytes32,
) -> SignerResult<Spend> {
    let member_config = MemberConfig::default();
    let fixed_conditions_hash =
        custom_member_hash(&member_config, ctx.tree_hash(fixed_spend.puzzle));
    let p2_singleton_hash = singleton_member_hash(&member_config, launcher_id, false);
    let full_puzzle_hash = m_of_n_hash(
        &member_config.with_top_level(true),
        1,
        vec![fixed_conditions_hash, p2_singleton_hash],
    );

    let nil_spend = Spend::new(NodePtr::NIL, NodePtr::NIL);
    let mut mips_spend = MipsSpend::new(nil_spend);
    mips_spend.members.insert(
        fixed_conditions_hash,
        InnerPuzzleSpend::new(0, Vec::new(), fixed_spend),
    );
    mips_spend.members.insert(
        full_puzzle_hash,
        InnerPuzzleSpend::m_of_n(
            0,
            Vec::new(),
            1,
            vec![fixed_conditions_hash, p2_singleton_hash],
        ),
    );
    mips_spend.spend(ctx, full_puzzle_hash).map_err(driver_err)
}

pub fn predict_presplit_cat(source_cat: &Cat, p2_puzzle_hash: Bytes32, offer_amount: u64) -> Cat {
    source_cat.child(p2_puzzle_hash, offer_amount)
}

pub fn vault_change_puzzle_hash(launcher_id: Bytes32) -> Bytes32 {
    singleton_member_hash(
        &MemberConfig::default().with_top_level(true),
        launcher_id,
        false,
    )
    .into()
}

pub async fn build_presplit_split_spend_bundle(
    vault_ctx: &mut VaultSpendContext,
    coinset: &CoinsetClient,
    source_cats: &[Cat],
    change_puzzle_hash: Bytes32,
    p2_puzzle_hash: Bytes32,
    offer_amount: u64,
    change_amount: u64,
) -> SignerResult<(SpendBundle, Cat)> {
    let vault = coinset::fetch_latest_vault(
        coinset,
        vault_ctx.launcher_id,
        vault_ctx.inner_puzzle_hash,
    )
    .await?;
    let kms_key_id = vault_ctx.kms_key_id.clone();
    let kms_region = vault_ctx.kms_region.clone();
    build_presplit_split_spend_bundle_with_vault(
        vault_ctx,
        source_cats,
        change_puzzle_hash,
        p2_puzzle_hash,
        offer_amount,
        change_amount,
        vault,
        move |message| {
            Box::pin(async move {
                crate::vault::spend::sign_vault_fast_forward_digest(
                    &kms_key_id,
                    &kms_region,
                    message,
                )
                .await
            })
        },
    )
    .await
}

pub fn validate_presplit_source_cats(source_cat_count: usize) -> SignerResult<()> {
    if source_cat_count != 1 {
        return Err(SignerError::PresplitRequiresSingleSourceCat);
    }
    Ok(())
}

pub(crate) async fn build_presplit_split_spend_bundle_with_vault<F, Fut>(
    vault_ctx: &mut VaultSpendContext,
    source_cats: &[Cat],
    change_puzzle_hash: Bytes32,
    p2_puzzle_hash: Bytes32,
    offer_amount: u64,
    change_amount: u64,
    vault: Vault,
    sign_digest: F,
) -> SignerResult<(SpendBundle, Cat)>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = SignerResult<chia_secp::R1Signature>>,
{
    validate_presplit_source_cats(source_cats.len())?;
    let source_cat = source_cats[0];
    let presplit_cat = predict_presplit_cat(&source_cat, p2_puzzle_hash, offer_amount);

    let mut ctx = SpendContext::new();
    let mut conditions = Conditions::new();
    if change_amount > 0 {
        let change_memos = ctx.hint(change_puzzle_hash).map_err(driver_err)?;
        conditions = conditions.create_coin(change_puzzle_hash, change_amount, change_memos);
    }
    conditions = conditions.create_coin(p2_puzzle_hash, offer_amount, Memos::None);

    let delegated = ctx.delegated_spend(conditions).map_err(driver_err)?;
    let nonce = vault_ctx
        .infer_nonce_for_p2_hash(source_cat.info.p2_puzzle_hash)
        .ok_or(SignerError::Driver(
            "failed to infer vault nonce for cat p2 puzzle hash".to_string(),
        ))?;
    let inner_spend = build_vault_cat_inner_spend(
        &mut ctx,
        delegated,
        vault_ctx,
        nonce,
        source_cat.info.p2_puzzle_hash.into(),
    )?;
    Cat::spend_all(&mut ctx, &[CatSpend::new(source_cat, inner_spend)]).map_err(driver_err)?;
    crate::vault::spend::append_vault_singleton_spend_for_vault(
        &mut ctx,
        vault_ctx,
        &vault,
        sign_digest,
    )
    .await?;
    Ok((SpendBundle::new(ctx.take(), chia_bls::Signature::default()), presplit_cat))
}

pub async fn build_offer_from_presplit_cat(
    presplit_cat: Cat,
    launcher_id: Bytes32,
    requested_payments: RequestedPayments,
    requested_asset_info: AssetInfo,
    offer_amount: u64,
    expires_at: Option<u64>,
) -> SignerResult<(String, String, String)> {
    let mut ctx = SpendContext::new();
    let fixed_spend = build_fixed_presplit_conditions_spend(
        &mut ctx,
        &requested_payments,
        &requested_asset_info,
        offer_amount,
        expires_at,
    )?;
    let inner_spend =
        build_presplit_conditions_inner_spend(&mut ctx, fixed_spend, launcher_id)?;
    Cat::spend_all(&mut ctx, &[CatSpend::new(presplit_cat, inner_spend)]).map_err(driver_err)?;
    let input_spend_bundle = SpendBundle::new(ctx.take(), chia_bls::Signature::default());

    let offer_nonce = Offer::nonce(vec![presplit_cat.coin.coin_id()]);
    let mut allocator = Allocator::new();
    let offer = Offer::from_input_spend_bundle(
        &mut allocator,
        input_spend_bundle.clone(),
        requested_payments,
        requested_asset_info,
    )
    .map_err(driver_err)?;
    let offer_spend_bundle = offer.to_spend_bundle(&mut ctx).map_err(driver_err)?;
    let offer_text = encode_offer(&offer_spend_bundle).map_err(driver_err)?;
    let spend_bundle_hex = coinset::spend_bundle_hex(&offer_spend_bundle)?;
    Ok((
        offer_text,
        spend_bundle_hex,
        hex::encode(offer_nonce),
    ))
}

pub async fn fetch_presplit_cat_by_id(
    coinset: &CoinsetClient,
    coin_id: Bytes32,
) -> SignerResult<Cat> {
    let response = coinset
        .get_coin_record_by_name(coin_id)
        .await
        .map_err(coinset_err)?;
    let Some(record) = response.coin_record else {
        return Err(SignerError::PresplitCoinNotFound);
    };
    if record.spent {
        return Err(SignerError::PresplitCoinNotFound);
    }
    let parent_response = coinset
        .get_coin_record_by_name(record.coin.parent_coin_info)
        .await
        .map_err(coinset_err)?;
    let Some(parent_record) = parent_response.coin_record else {
        return Err(SignerError::PresplitCoinNotFound);
    };
    let solution_response = coinset
        .get_puzzle_and_solution(
            parent_record.coin.coin_id(),
            Some(parent_record.spent_block_index),
        )
        .await
        .map_err(coinset_err)?;
    let Some(parent_spend) = solution_response.coin_solution else {
        return Err(SignerError::PresplitCoinNotFound);
    };
    parse_presplit_cat_from_parent(record.coin, &parent_spend)
}

fn parse_presplit_cat_from_parent(coin: Coin, parent_spend: &CoinSpend) -> SignerResult<Cat> {
    let mut allocator = Allocator::new();
    let parent_puzzle_ptr = clvmr::serde::node_from_bytes(
        &mut allocator,
        parent_spend.puzzle_reveal.as_ref(),
    )
    .map_err(|err| SignerError::Driver(err.to_string()))?;
    let parent_solution_ptr = clvmr::serde::node_from_bytes(
        &mut allocator,
        parent_spend.solution.as_ref(),
    )
    .map_err(|err| SignerError::Driver(err.to_string()))?;
    let parent_puzzle = chia_sdk_driver::Puzzle::parse(&allocator, parent_puzzle_ptr);
    let children = Cat::parse_children(
        &mut allocator,
        parent_spend.coin,
        parent_puzzle,
        parent_solution_ptr,
    )
    .map_err(|err| SignerError::Driver(err.to_string()))?;
    let Some(children) = children else {
        return Err(SignerError::PresplitCoinNotFound);
    };
    children
        .into_iter()
        .find(|cat| cat.coin.coin_id() == coin.coin_id())
        .ok_or(SignerError::PresplitCoinNotFound)
}

fn driver_err(err: DriverError) -> SignerError {
    SignerError::Driver(err.to_string())
}

fn coinset_err(err: reqwest::Error) -> SignerError {
    SignerError::Coinset(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::members::p2_conditions_or_singleton_puzzle_hash;
    use chia_sdk_driver::{AssetInfo, RequestedPayments};
    use chia_sdk_types::Condition;
    use clvm_traits::FromClvm;
    use chia_sdk_types::run_puzzle;

    #[test]
    fn should_presplit_when_change_and_flag_enabled() {
        assert!(should_presplit(5000, 1000, true));
        assert!(!should_presplit(1000, 1000, true));
        assert!(!should_presplit(5000, 1000, false));
    }

    #[test]
    fn p2_conditions_or_singleton_hash_is_deterministic() {
        let launcher_id = Bytes32::new([0xcc; 32]);
        let mut ctx = SpendContext::new();
        let fixed_spend = ctx
            .delegated_spend(
                Conditions::new().create_coin(Bytes32::new([0xab; 32]), 1, Memos::None),
            )
            .expect("fixed spend");
        let puzzle_hash = ctx.tree_hash(fixed_spend.puzzle);
        let hashes = p2_conditions_or_singleton_puzzle_hash(puzzle_hash, launcher_id);
        assert_ne!(hashes.puzzle_hash.to_bytes(), [0u8; 32]);
        assert_ne!(hashes.fixed_conditions_hash.to_bytes(), [0u8; 32]);
    }

    #[test]
    fn presplit_requires_single_source_cat() {
        let err = validate_presplit_source_cats(2).unwrap_err();
        assert!(matches!(err, SignerError::PresplitRequiresSingleSourceCat));
    }

    #[test]
    fn fixed_presplit_conditions_include_settlement_output() {
        let mut ctx = SpendContext::new();
        let requested_payments = RequestedPayments::new();
        let asset_info = AssetInfo::new();
        let fixed_spend = build_fixed_presplit_conditions_spend(
            &mut ctx,
            &requested_payments,
            &asset_info,
            1000,
            None,
        )
        .expect("fixed spend");
        let output = run_puzzle(&mut ctx, fixed_spend.puzzle, fixed_spend.solution).expect("run puzzle");
        let conditions = Conditions::<NodePtr>::from_clvm(&ctx, output).expect("conditions");
        assert!(
            conditions
                .iter()
                .any(|condition| matches!(condition, Condition::CreateCoin(create)
                    if create.puzzle_hash == SETTLEMENT_PAYMENT_HASH.into() && create.amount == 1000))
        );
    }
}
