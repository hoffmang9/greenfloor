//! Maker watch-seed policy (ADR 0019). Shape classification lives on [`PostedOfferShape`].

use crate::hex::canonical_tx_id;
use crate::offer::types::{
    OfferCancelFields, OfferExecutionMode, PostedOfferShape, StoredOfferCancelMetadata,
};

/// Coin-id and p2 watches to seed for a posted maker (ADR 0019).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MakerWatchSeed {
    pub coin_ids: Vec<String>,
    pub p2s: Vec<String>,
}

fn watchable_coin_ids(ids: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
    let mut coin_ids: Vec<String> = ids
        .into_iter()
        .filter_map(|value| canonical_tx_id(value.as_ref()))
        .collect();
    coin_ids.sort();
    coin_ids.dedup();
    coin_ids
}

fn p2s_if_presplit(shape: PostedOfferShape, p2s: impl IntoIterator<Item = String>) -> Vec<String> {
    if shape.is_presplit() {
        p2s.into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect()
    } else {
        Vec::new()
    }
}

impl MakerWatchSeed {
    /// Watches from stored cancel metadata (heal path).
    #[must_use]
    pub fn from_metadata(meta: &StoredOfferCancelMetadata) -> Self {
        Self {
            coin_ids: watchable_coin_ids(meta.fields.input_coin_id.iter()),
            p2s: p2s_if_presplit(
                PostedOfferShape::from_metadata(meta),
                meta.fields.maker_puzzle_hash.clone(),
            ),
        }
    }

    /// Watches at offer post persist time.
    #[must_use]
    pub fn from_post_fields(
        execution_mode: Option<OfferExecutionMode>,
        cancel_fields: &OfferCancelFields,
        extra_coin_ids: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            coin_ids: watchable_coin_ids(
                extra_coin_ids
                    .into_iter()
                    .chain(cancel_fields.input_coin_id.clone()),
            ),
            p2s: p2s_if_presplit(
                PostedOfferShape::from_execution(
                    execution_mode,
                    cancel_fields.fixed_delegated_puzzle_hash.as_deref(),
                ),
                cancel_fields.maker_puzzle_hash.clone(),
            ),
        }
    }

    /// Dexie-heal watches: payload coin ids; p2s only when local metadata says presplit.
    #[must_use]
    pub fn from_dexie_heal(
        meta: Option<&StoredOfferCancelMetadata>,
        coin_ids: Vec<String>,
        payload_p2s: Vec<String>,
    ) -> Self {
        let shape = meta.map_or(PostedOfferShape::Direct, PostedOfferShape::from_metadata);
        Self {
            coin_ids: watchable_coin_ids(coin_ids),
            p2s: p2s_if_presplit(shape, payload_p2s),
        }
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

    #[test]
    fn dexie_heal_seeds_payload_p2_only_for_presplit() {
        let presplit = StoredOfferCancelMetadata {
            fields: OfferCancelFields::from_presplit_build(
                "aa".repeat(32),
                "cc".repeat(32),
                "dd".repeat(32),
            ),
            execution_mode: Some(OfferExecutionMode::PresplitExisting),
        };
        let seed = MakerWatchSeed::from_dexie_heal(
            Some(&presplit),
            vec!["ee".repeat(32)],
            vec!["ff".repeat(32)],
        );
        assert_eq!(seed.coin_ids, vec!["ee".repeat(32)]);
        assert_eq!(seed.p2s, vec!["ff".repeat(32)]);

        let direct = StoredOfferCancelMetadata {
            fields: OfferCancelFields::from_direct_build("aa".repeat(32), "bb".repeat(32)),
            execution_mode: Some(OfferExecutionMode::Direct),
        };
        let seed = MakerWatchSeed::from_dexie_heal(
            Some(&direct),
            vec!["ee".repeat(32)],
            vec!["ff".repeat(32)],
        );
        assert_eq!(seed.coin_ids, vec!["ee".repeat(32)]);
        assert!(seed.p2s.is_empty());

        let seed =
            MakerWatchSeed::from_dexie_heal(None, vec!["ee".repeat(32)], vec!["ff".repeat(32)]);
        assert!(seed.p2s.is_empty());
    }

    #[test]
    fn p2s_if_presplit_drops_empty_and_direct() {
        assert!(p2s_if_presplit(PostedOfferShape::Direct, ["aa".repeat(32)]).is_empty());
        assert_eq!(
            p2s_if_presplit(PostedOfferShape::Presplit, ["dd".repeat(32), String::new()]),
            vec!["dd".repeat(32)]
        );
    }

    #[test]
    fn watchable_coin_ids_normalizes_dedups_and_drops_invalid() {
        let raw = [
            format!("0x{}", "aa".repeat(32)),
            "aa".repeat(32),
            "not-a-coin".to_string(),
            String::new(),
            "bb".repeat(32),
        ];
        assert_eq!(
            watchable_coin_ids(raw),
            vec!["aa".repeat(32), "bb".repeat(32)]
        );

        let fields = OfferCancelFields::from_direct_build("aa".repeat(32), "bb".repeat(32));
        let seed = MakerWatchSeed::from_post_fields(
            Some(OfferExecutionMode::Direct),
            &fields,
            ["short".to_string(), format!("0x{}", "cc".repeat(32))],
        );
        assert_eq!(seed.coin_ids, vec!["aa".repeat(32), "cc".repeat(32)]);

        let seed = MakerWatchSeed::from_dexie_heal(
            None,
            vec!["ee".repeat(32), "not-hex".to_string(), "ee".repeat(32)],
            vec!["ff".repeat(32)],
        );
        assert_eq!(seed.coin_ids, vec!["ee".repeat(32)]);
    }
}
