//! Maker watch-seed policy (ADR 0019). Shape classification lives on [`PostedOfferShape`].

use crate::hex::normalize_hex_id;
use crate::offer::types::{
    OfferCancelFields, OfferExecutionMode, PostedOfferShape, StoredOfferCancelMetadata,
};

/// Coin-id and p2 watches to seed for a posted maker (ADR 0019).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MakerWatchSeed {
    pub coin_ids: Vec<String>,
    pub p2s: Vec<String>,
}

impl MakerWatchSeed {
    /// Watches from stored cancel metadata (heal path).
    #[must_use]
    pub fn from_metadata(meta: &StoredOfferCancelMetadata) -> Self {
        let mut coin_ids = Vec::new();
        if let Some(coin) = meta
            .fields
            .input_coin_id
            .as_deref()
            .map(normalize_hex_id)
            .filter(|value| value.len() == 64)
        {
            coin_ids.push(coin);
        }
        let p2s = if PostedOfferShape::from_metadata(meta).is_presplit() {
            meta.fields
                .maker_puzzle_hash
                .clone()
                .into_iter()
                .filter(|value| !value.trim().is_empty())
                .collect()
        } else {
            Vec::new()
        };
        Self { coin_ids, p2s }
    }

    /// Watches at offer post persist time.
    #[must_use]
    pub fn from_post_fields(
        execution_mode: Option<OfferExecutionMode>,
        cancel_fields: &OfferCancelFields,
        extra_coin_ids: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut coin_ids: Vec<String> = extra_coin_ids.into_iter().collect();
        if let Some(input) = cancel_fields.input_coin_id.clone() {
            coin_ids.push(input);
        }
        coin_ids.sort();
        coin_ids.dedup();
        let p2s = if PostedOfferShape::from_execution(
            execution_mode,
            cancel_fields.fixed_delegated_puzzle_hash.as_deref(),
        )
        .is_presplit()
        {
            cancel_fields
                .maker_puzzle_hash
                .clone()
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };
        Self { coin_ids, p2s }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_execution_never_seeds_p2() {
        let fields = OfferCancelFields::from_direct_build("aa".repeat(32), "bb".repeat(32));
        let seed = MakerWatchSeed::from_post_fields(Some(OfferExecutionMode::Direct), &fields, []);
        assert!(seed.p2s.is_empty());
        assert_eq!(seed.coin_ids, vec!["aa".repeat(32)]);
    }

    #[test]
    fn presplit_execution_seeds_maker_p2() {
        let fields = OfferCancelFields::from_presplit_build(
            "aa".repeat(32),
            "cc".repeat(32),
            "dd".repeat(32),
        );
        let seed =
            MakerWatchSeed::from_post_fields(Some(OfferExecutionMode::PresplitNew), &fields, []);
        assert_eq!(seed.p2s, vec!["dd".repeat(32)]);
    }
}
