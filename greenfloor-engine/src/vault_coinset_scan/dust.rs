use std::collections::HashMap;

use chia_protocol::Bytes32;
use chia_sdk_coinset::CoinsetClient;
use chia_sdk_driver::Cat;

use crate::coinset::{chunk_values, list_unspent_cats_by_ids};
use crate::error::SignerResult;
use crate::hex::{hex_to_bytes32, normalize_hex_id};
use crate::operator_log::LogContext;
use crate::vault_coinset_scan::types::{CoinKind, CoinRow};

const DUST_LINEAGE_FILTER_CHUNK: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DustCoin {
    pub coin_id: String,
    pub amount: u64,
}

impl DustCoin {
    /// Project scan/report metadata from a spend-ready CAT.
    #[must_use]
    pub fn from_cat(cat: &Cat) -> Self {
        Self {
            coin_id: normalize_hex_id(&hex::encode(cat.coin.coin_id())),
            amount: cat.coin.amount,
        }
    }
}

/// Lineage-proven dust coin: spend-ready [`Cat`] retained after lineage validation.
#[derive(Debug, Clone)]
pub struct ProvenDustCoin {
    cat: Cat,
}

impl ProvenDustCoin {
    /// Wrap a spend-ready CAT that already passed lineage resolution.
    #[must_use]
    pub fn from_cat(cat: Cat) -> Self {
        Self { cat }
    }

    /// Validate scan dust metadata against a spend-ready CAT, then retain the CAT only.
    ///
    /// # Errors
    ///
    /// Returns [`SignerError::ProvenDustCoinMismatch`] when `dust` and `cat` disagree.
    pub fn from_lineage(dust: &DustCoin, cat: Cat) -> SignerResult<Self> {
        let projected = DustCoin::from_cat(&cat);
        if dust.coin_id != projected.coin_id || dust.amount != projected.amount {
            return Err(crate::error::SignerError::ProvenDustCoinMismatch);
        }
        Ok(Self { cat })
    }

    pub fn cat(&self) -> &Cat {
        &self.cat
    }

    pub fn into_cat(self) -> Cat {
        self.cat
    }

    /// Scan/report projection for JSON batch entries and orphan lists.
    #[must_use]
    pub fn dust_coin(&self) -> DustCoin {
        DustCoin::from_cat(&self.cat)
    }
}

#[derive(Debug, Clone)]
pub struct DustCombineBatch {
    pub items: Vec<ProvenDustCoin>,
}

impl DustCombineBatch {
    #[must_use]
    pub fn total_amount(&self) -> u64 {
        self.items.iter().map(|item| item.cat.coin.amount).sum()
    }

    /// Coin ids for batch items in spend order.
    ///
    /// # Errors
    ///
    /// Returns an error when any coin id cannot be encoded (should not happen for valid cats).
    pub fn coin_ids(&self) -> SignerResult<Vec<Bytes32>> {
        Ok(self
            .items
            .iter()
            .map(|item| item.cat.coin.coin_id())
            .collect())
    }

    #[must_use]
    pub fn cats(&self) -> Vec<Cat> {
        self.items.iter().map(|item| item.cat).collect()
    }
}

#[derive(Debug, Clone)]
pub struct DustBatchPlan {
    pub combinable_batches: Vec<DustCombineBatch>,
    /// Dust coins that do not fill a full combine batch (batch-size orphans).
    pub uncombinable: Vec<DustCoin>,
}

#[derive(Debug, Clone)]
pub struct DustPlan {
    pub scan_dust_count: usize,
    pub batches: DustBatchPlan,
    pub lineage_excluded: Vec<DustCoin>,
}

#[must_use]
pub fn dust_coins_from_scan(coins: &[CoinRow], dust_threshold_mojos: u64) -> Vec<DustCoin> {
    let mut out = Vec::new();
    for row in coins {
        if row.kind != CoinKind::Cat {
            continue;
        }
        if row.spent_block_index != 0 {
            continue;
        }
        if row.amount == 0 || row.amount >= dust_threshold_mojos {
            continue;
        }
        let coin_id = normalize_hex_id(&row.coin_id);
        if coin_id.is_empty() {
            continue;
        }
        out.push(DustCoin {
            coin_id,
            amount: row.amount,
        });
    }
    out
}

/// Resolve spend-ready [`Cat`] values for dust coins (same bar as `list_unspent_cats`).
///
/// Returns proven dust/cat pairs in scan order and coins that failed lineage.
///
/// # Errors
///
/// Returns an error if Coinset lineage lookups fail.
pub async fn prove_dust_coins_lineage(
    client: &CoinsetClient,
    dust_coins: &[DustCoin],
) -> SignerResult<(Vec<ProvenDustCoin>, Vec<DustCoin>)> {
    if dust_coins.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let coin_ids: Vec<Bytes32> = dust_coins
        .iter()
        .map(|coin| hex_to_bytes32(&coin.coin_id))
        .collect::<SignerResult<_>>()?;

    let mut cat_by_id = HashMap::new();
    for chunk in chunk_values(&coin_ids, DUST_LINEAGE_FILTER_CHUNK) {
        for cat in list_unspent_cats_by_ids(client, chunk.as_slice()).await? {
            cat_by_id.insert(normalize_hex_id(&hex::encode(cat.coin.coin_id())), cat);
        }
    }

    let mut proven = Vec::with_capacity(cat_by_id.len());
    let mut lineage_excluded = Vec::new();
    for coin in dust_coins {
        if let Some(cat) = cat_by_id.remove(&coin.coin_id) {
            proven.push(ProvenDustCoin::from_lineage(coin, cat)?);
        } else {
            lineage_excluded.push(coin.clone());
        }
    }

    crate::trace_event!(
        DEBUG,
        LogContext::COINSET,
        "dust_lineage_filter",
        {
            dust_coin_count = dust_coins.len(),
            lineage_proven_count = proven.len(),
            lineage_excluded_count = lineage_excluded.len(),
        };
        "filtered vault dust coins by spend-ready CAT lineage"
    );

    Ok((proven, lineage_excluded))
}

/// Discover dust from a vault scan, keep lineage-proven coins, and plan combine batches.
///
/// # Errors
///
/// Returns an error if Coinset lineage lookups fail.
pub async fn plan_dust_from_scan_with_lineage(
    client: &CoinsetClient,
    coins: &[CoinRow],
    dust_threshold_mojos: u64,
    max_input_coins: usize,
) -> SignerResult<DustPlan> {
    let dust_coins = dust_coins_from_scan(coins, dust_threshold_mojos);
    let scan_dust_count = dust_coins.len();
    let (proven, lineage_excluded) = prove_dust_coins_lineage(client, &dust_coins).await?;
    Ok(DustPlan {
        scan_dust_count,
        batches: plan_dust_batches(&proven, max_input_coins),
        lineage_excluded,
    })
}

#[must_use]
pub fn plan_dust_batches(proven: &[ProvenDustCoin], batch_size: usize) -> DustBatchPlan {
    let size = batch_size.max(2);
    if proven.is_empty() {
        return DustBatchPlan {
            combinable_batches: Vec::new(),
            uncombinable: Vec::new(),
        };
    }
    let full_batches = proven.len() / size;
    let combinable_batches = proven
        .chunks(size)
        .take(full_batches)
        .map(|chunk| DustCombineBatch {
            items: chunk.to_vec(),
        })
        .collect();
    let uncombinable = proven[full_batches * size..]
        .iter()
        .map(ProvenDustCoin::dust_coin)
        .collect();
    DustBatchPlan {
        combinable_batches,
        uncombinable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coinset::test_support::cat_with_amount;
    use crate::vault_coinset_scan::types::CoinRow;

    fn cat_row(coin_id: &str, amount: u64, spent: u64) -> CoinRow {
        CoinRow {
            coin_id: coin_id.to_string(),
            puzzle_hash: "b".repeat(64),
            parent_coin_info: "c".repeat(64),
            amount,
            confirmed_block_index: 1,
            spent_block_index: spent,
            discovered_nonces: vec![1],
            discovered_by_puzzle_hash: true,
            discovered_by_hint: false,
            kind: CoinKind::Cat,
            cat_asset_id: Some("d".repeat(64)),
            cat_symbols: vec![],
        }
    }

    fn proven_dust(coin_id: &str, amount: u64) -> ProvenDustCoin {
        let mut cat = cat_with_amount(amount);
        cat.coin = chia_protocol::Coin::new(
            hex_to_bytes32(coin_id).expect("coin id"),
            cat.coin.puzzle_hash,
            amount,
        );
        ProvenDustCoin::from_cat(cat)
    }

    #[test]
    fn dust_coins_from_scan_filters_spent_and_threshold() {
        let cat = "a".repeat(64);
        let coins = vec![
            cat_row(&cat, 500, 0),
            cat_row(&"b".repeat(64), 1000, 0),
            cat_row(&"c".repeat(64), 100, 1),
            cat_row(&"d".repeat(64), 1, 0),
        ];
        let got = dust_coins_from_scan(&coins, 1000);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].coin_id, cat);
        assert_eq!(got[0].amount, 500);
        assert_eq!(got[1].amount, 1);
    }

    #[test]
    fn proven_dust_coin_from_lineage_rejects_mismatched_coin_id_or_amount() {
        let mut cat = cat_with_amount(100);
        cat.coin = chia_protocol::Coin::new(
            hex_to_bytes32(&"a".repeat(64)).expect("coin id"),
            cat.coin.puzzle_hash,
            100,
        );
        let err = ProvenDustCoin::from_lineage(
            &DustCoin {
                coin_id: "b".repeat(64),
                amount: 100,
            },
            cat,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            crate::error::SignerError::ProvenDustCoinMismatch
        ));

        let mut cat = cat_with_amount(50);
        cat.coin = chia_protocol::Coin::new(
            hex_to_bytes32(&"a".repeat(64)).expect("coin id"),
            cat.coin.puzzle_hash,
            50,
        );
        let err = ProvenDustCoin::from_lineage(
            &DustCoin {
                coin_id: "a".repeat(64),
                amount: 100,
            },
            cat,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            crate::error::SignerError::ProvenDustCoinMismatch
        ));
    }

    #[test]
    fn plan_dust_batches_keeps_orphans_out_of_combinable_batches() {
        let proven: Vec<ProvenDustCoin> = (0..5)
            .map(|i| proven_dust(&format!("{i:064x}"), 1))
            .collect();
        let plan = plan_dust_batches(&proven, 2);
        assert_eq!(plan.combinable_batches.len(), 2);
        assert_eq!(plan.combinable_batches[0].items.len(), 2);
        assert_eq!(plan.combinable_batches[1].items.len(), 2);
        assert_eq!(plan.uncombinable.len(), 1);
        assert_eq!(plan.uncombinable[0].coin_id, proven[4].dust_coin().coin_id);
    }
}
