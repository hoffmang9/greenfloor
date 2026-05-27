use std::collections::HashSet;

use chia_bls::{PublicKey, SecretKey};
use chia_protocol::{Bytes32, Coin, SpendBundle};
use chia_puzzle_types::Memos;
use chia_sdk_driver::{Action, Id, Relation, SpendContext, Spends};
use chia_sdk_utils::select_coins;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::bls::keys::synthetic_secret_keys_for_puzzle_hashes;
use crate::bls::signing::sign_coin_spends;
use crate::coinset::{client_for_network, decode_receive_address, list_unspent_xch};
use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone, Deserialize)]
pub struct BlsXchCoinOpRequest {
    pub receive_address: String,
    pub op_type: String,
    pub size_base_units: u64,
    pub op_count: u32,
    #[serde(default)]
    pub target_total_base_units: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlsXchCoinOpResult {
    pub spend_bundle_hex: String,
}

pub(crate) fn plan_xch_additions(
    op_type: &str,
    size_base_units: u64,
    op_count: u32,
    target_total_base_units: u64,
    receive_puzzle_hash: Bytes32,
    selected_total: u64,
) -> SignerResult<Vec<(Bytes32, u64)>> {
    let mut target_total = target_total_base_units;
    if target_total == 0 {
        target_total = size_base_units.saturating_mul(u64::from(op_count));
    }
    let normalized_op = op_type.trim().to_ascii_lowercase();
    if normalized_op != "split" && normalized_op != "combine" {
        return Err(SignerError::UnsupportedOperationType);
    }
    if size_base_units == 0 || op_count == 0 || target_total == 0 {
        return Err(SignerError::InvalidPlanValues);
    }
    if selected_total < target_total {
        return Err(SignerError::InsufficientSelectedCoinTotal);
    }

    let op_count_usize = op_count as usize;
    let mut outputs = Vec::with_capacity(op_count_usize + 1);
    for _ in 0..op_count {
        outputs.push((receive_puzzle_hash, size_base_units));
    }
    let change = selected_total.saturating_sub(target_total);
    if change > 0 {
        outputs.push((receive_puzzle_hash, change));
    }
    Ok(outputs)
}

pub async fn build_bls_xch_coin_op_spend_bundle(
    network: &str,
    master_sk: &SecretKey,
    request: BlsXchCoinOpRequest,
) -> SignerResult<BlsXchCoinOpResult> {
    let client = client_for_network(network)?;
    let receive_address = request.receive_address.trim();
    let receive_puzzle_hash = decode_receive_address(receive_address)?;

    let xch_coins = list_unspent_xch(&client, receive_address).await?;
    if xch_coins.is_empty() {
        return Err(SignerError::NoUnspentXchCoins);
    }

    let mut target_total = request.target_total_base_units;
    if target_total == 0 {
        target_total = request
            .size_base_units
            .saturating_mul(u64::from(request.op_count));
    }

    let selected = select_coins(xch_coins, target_total).map_err(|_| SignerError::XchCoinSelectionFailed)?;
    let selected_total: u64 = selected.iter().map(|coin| coin.amount).sum::<u64>();
    let outputs = plan_xch_additions(
        &request.op_type,
        request.size_base_units,
        request.op_count,
        request.target_total_base_units,
        receive_puzzle_hash,
        selected_total,
    )?;

    let required_puzzle_hashes: HashSet<Bytes32> =
        selected.iter().map(|coin| coin.puzzle_hash).collect();
    let synthetic_sks =
        synthetic_secret_keys_for_puzzle_hashes(master_sk, &required_puzzle_hashes, None)?;
    let synthetic_pks: IndexMap<Bytes32, PublicKey> = synthetic_sks
        .iter()
        .map(|(puzzle_hash, sk)| (*puzzle_hash, sk.public_key()))
        .collect();

    let mut ctx = SpendContext::new();
    let mut spends = Spends::new(receive_puzzle_hash);
    for coin in selected {
        spends.add(coin);
    }
    let actions: Vec<Action> = outputs
        .into_iter()
        .map(|(puzzle_hash, amount)| Action::send(Id::Xch, puzzle_hash, amount, Memos::None))
        .collect();
    let deltas = spends.apply(&mut ctx, &actions)?;
    spends.finish_with_keys(&mut ctx, &deltas, Relation::None, &synthetic_pks)?;
    let coin_spends = ctx.take();
    let signature = sign_coin_spends(network, &coin_spends, &synthetic_sks)?;
    let spend_bundle = SpendBundle::new(coin_spends, signature);
    Ok(BlsXchCoinOpResult {
        spend_bundle_hex: crate::coinset::spend_bundle_hex(&spend_bundle)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_protocol::Bytes32;

    #[test]
    fn plan_xch_additions_split_includes_change() {
        let receive = Bytes32::new([0x11; 32]);
        let outputs =
            plan_xch_additions("split", 10, 2, 0, receive, 25).expect("valid additions");
        assert_eq!(outputs.len(), 3);
        assert_eq!(outputs[0], (receive, 10));
        assert_eq!(outputs[1], (receive, 10));
        assert_eq!(outputs[2], (receive, 5));
    }

    #[test]
    fn plan_xch_additions_rejects_insufficient_selected_total() {
        let err = plan_xch_additions("split", 100, 2, 0, Bytes32::default(), 10).unwrap_err();
        assert!(matches!(err, SignerError::InsufficientSelectedCoinTotal));
    }
}
