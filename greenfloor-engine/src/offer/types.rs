use chia_protocol::Bytes32;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{SignerError, SignerResult};

fn default_bake_expiry_into_conditions() -> bool {
    true
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct CreateOfferRequest {
    pub receive_address: String,
    pub offer_asset_id: String,
    pub offer_amount: u64,
    pub request_asset_id: String,
    pub request_amount: u64,
    #[serde(
        default,
        deserialize_with = "deserialize_coin_ids",
        serialize_with = "serialize_coin_ids"
    )]
    pub offer_coin_ids: Vec<Bytes32>,
    #[serde(
        default,
        deserialize_with = "deserialize_coin_ids",
        serialize_with = "serialize_coin_ids"
    )]
    pub presplit_coin_ids: Vec<Bytes32>,
    pub split_input_coins: bool,
    pub broadcast_split: bool,
    pub expires_at: Option<u64>,
    /// When false (stable soft-expiry), listing expiry is kept but not baked into
    /// presplit fixed CONDITIONS. Direct path still uses [`Self::expires_at`].
    #[serde(default = "default_bake_expiry_into_conditions")]
    pub bake_expiry_into_conditions: bool,
    /// Explicit offer nonce for [`OfferInput::PresplitExisting`] when source coin ids
    /// are unavailable (reuse after soft listing expiry).
    #[serde(
        default,
        deserialize_with = "deserialize_optional_coin_id",
        serialize_with = "serialize_optional_coin_id"
    )]
    pub offer_nonce: Option<Bytes32>,
}

/// Reuse an unspent soft-expiry maker via `PresplitExisting`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PresplitMakerReuse {
    pub coin_id: String,
    pub offer_nonce: String,
}

impl PresplitMakerReuse {
    /// Non-empty coin id and nonce — same gate create uses before `PreferExisting`.
    #[must_use]
    pub fn is_usable(&self) -> bool {
        !self.coin_id.trim().is_empty() && !self.offer_nonce.trim().is_empty()
    }
}

/// Maker reuse that create will actually apply (non-empty coin id + nonce).
#[must_use]
pub fn effective_maker_reuse(reuse: Option<&PresplitMakerReuse>) -> Option<&PresplitMakerReuse> {
    reuse.filter(|r| r.is_usable())
}

/// Shared offer fields parsed from CLI/API input.
#[derive(Debug, Clone)]
pub(crate) struct OfferTerms {
    pub receive_address: String,
    pub offer_asset_id: String,
    pub offer_amount: u64,
    pub request_asset_id: String,
    pub request_amount: u64,
    pub expires_at: Option<u64>,
    pub bake_expiry_into_conditions: bool,
}

impl OfferTerms {
    /// Expiry seconds included in presplit fixed CONDITIONS (None for soft-expiry makers).
    #[must_use]
    pub(crate) fn conditions_expires_at(&self) -> Option<u64> {
        if self.bake_expiry_into_conditions {
            self.expires_at
        } else {
            None
        }
    }
}

/// Typed offer path after parsing [`CreateOfferRequest`].
///
/// # Execution vs input mode
///
/// [`OfferInput::PresplitNew`] means the operator enabled `--split-input-coins`.
/// Planning still selects the direct offer path when a **single** selected CAT
/// already equals `--offer-amount` exactly (no change to split off). Multi-coin
/// exact sums require combine or the split path — Direct cancel metadata stores
/// one maker input. In the single-coin case execution uses the direct assembler
/// and `execution_mode` is [`OfferExecutionMode::Direct`], even though the input
/// variant is `PresplitNew`.
#[derive(Debug, Clone)]
pub(crate) enum OfferInput {
    Direct {
        terms: OfferTerms,
        offer_coin_ids: Vec<Bytes32>,
    },
    PresplitNew {
        terms: OfferTerms,
        offer_coin_ids: Vec<Bytes32>,
        broadcast_split: bool,
    },
    PresplitExisting {
        terms: OfferTerms,
        presplit_coin_id: Bytes32,
        source_coin_ids: Vec<Bytes32>,
        /// When set, used instead of deriving nonce from [`Self::PresplitExisting::source_coin_ids`].
        offer_nonce: Option<Bytes32>,
    },
}

impl OfferInput {
    pub fn terms(&self) -> &OfferTerms {
        match self {
            Self::Direct { terms, .. }
            | Self::PresplitNew { terms, .. }
            | Self::PresplitExisting { terms, .. } => terms,
        }
    }
}

impl TryFrom<CreateOfferRequest> for OfferInput {
    type Error = SignerError;

    fn try_from(request: CreateOfferRequest) -> SignerResult<Self> {
        let terms = OfferTerms {
            receive_address: request.receive_address,
            offer_asset_id: request.offer_asset_id,
            offer_amount: request.offer_amount,
            request_asset_id: request.request_asset_id,
            request_amount: request.request_amount,
            expires_at: request.expires_at,
            bake_expiry_into_conditions: request.bake_expiry_into_conditions,
        };

        if !request.presplit_coin_ids.is_empty() {
            if request.presplit_coin_ids.len() != 1 {
                return Err(SignerError::PresplitOfferRequiresSingleCoin);
            }
            return Ok(Self::PresplitExisting {
                terms,
                presplit_coin_id: request.presplit_coin_ids[0],
                source_coin_ids: request.offer_coin_ids,
                offer_nonce: request.offer_nonce,
            });
        }

        if request.split_input_coins {
            Ok(Self::PresplitNew {
                terms,
                offer_coin_ids: request.offer_coin_ids,
                broadcast_split: request.broadcast_split,
            })
        } else {
            Ok(Self::Direct {
                terms,
                offer_coin_ids: request.offer_coin_ids,
            })
        }
    }
}

pub(crate) fn deserialize_coin_ids<'de, D>(deserializer: D) -> Result<Vec<Bytes32>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Vec<String> = Vec::deserialize(deserializer)?;
    raw.into_iter()
        .map(|value| crate::hex::hex_to_bytes32(&value).map_err(serde::de::Error::custom))
        .collect()
}

pub(crate) fn deserialize_bytes32<'de, D>(deserializer: D) -> Result<Bytes32, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    crate::hex::hex_to_bytes32(&raw).map_err(serde::de::Error::custom)
}

pub(crate) fn serialize_coin_ids<S>(values: &[Bytes32], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let encoded: Vec<String> = values.iter().map(hex::encode).collect();
    encoded.serialize(serializer)
}

pub(crate) fn serialize_bytes32<S>(value: &Bytes32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&hex::encode(value))
}

fn deserialize_optional_coin_id<'de, D>(deserializer: D) -> Result<Option<Bytes32>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    match raw {
        None => Ok(None),
        Some(value) if value.trim().is_empty() => Ok(None),
        Some(value) => crate::hex::hex_to_bytes32(&value)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

#[allow(clippy::ref_option)] // serde `serialize_with` always passes `&Option<T>`
fn serialize_optional_coin_id<S>(value: &Option<Bytes32>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(id) => serializer.serialize_some(&hex::encode(id)),
        None => serializer.serialize_none(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OfferExecutionMode {
    Direct,
    PresplitNew,
    PresplitExisting,
}

impl std::fmt::Display for OfferExecutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Direct => "direct",
            Self::PresplitNew => "presplit_new",
            Self::PresplitExisting => "presplit_existing",
        })
    }
}

impl OfferExecutionMode {
    #[must_use]
    pub fn parse_db(value: &str) -> Option<Self> {
        match value.trim() {
            "direct" => Some(Self::Direct),
            "presplit_new" => Some(Self::PresplitNew),
            "presplit_existing" => Some(Self::PresplitExisting),
            _ => None,
        }
    }
}

/// Cancel hints persisted at offer post time (Direct and presplit execution modes).
///
/// Stored in `offer_state` columns `cancel_input_coin_id` (maker input coin id for
/// every execution mode), `fixed_delegated_puzzle_hash` (presplit only), and
/// `maker_puzzle_hash`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OfferCancelFields {
    pub input_coin_id: Option<String>,
    /// Fixed CONDITIONS tree hash (cancel/reclaim verification). Not an on-chain coin p2.
    pub fixed_delegated_puzzle_hash: Option<String>,
    /// On-chain maker coin puzzle hash (CAT outer or XCH p2) for WS / coin-ops watches.
    pub maker_puzzle_hash: Option<String>,
}

impl OfferCancelFields {
    #[must_use]
    pub fn from_presplit_build(
        input_coin_id: String,
        fixed_delegated_puzzle_hash: String,
        maker_puzzle_hash: String,
    ) -> Self {
        Self {
            input_coin_id: Some(input_coin_id),
            fixed_delegated_puzzle_hash: Some(fixed_delegated_puzzle_hash),
            maker_puzzle_hash: Some(maker_puzzle_hash),
        }
    }

    /// Direct-path cancel hints: maker input coin id + on-chain puzzle hash (no fixed CONDITIONS).
    #[must_use]
    pub fn from_direct_build(input_coin_id: String, maker_puzzle_hash: String) -> Self {
        Self {
            input_coin_id: Some(input_coin_id),
            fixed_delegated_puzzle_hash: None,
            maker_puzzle_hash: Some(maker_puzzle_hash),
        }
    }
}

/// Cancel hints persisted at offer post time (`offer_state` row).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredOfferCancelMetadata {
    pub fields: OfferCancelFields,
    pub execution_mode: Option<OfferExecutionMode>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CreateOfferResult {
    pub offer: String,
    pub spend_bundle_hex: String,
    /// Source CAT coin IDs for direct and presplit-new paths. Empty for presplit-existing.
    pub selected_coin_ids: Vec<String>,
    pub offer_nonce: String,
    pub execution_mode: OfferExecutionMode,
    pub split_spend_bundle_hex: Option<String>,
    /// Presplit offer coin ID for presplit-new and presplit-existing paths.
    pub presplit_coin_id: Option<String>,
    pub split_broadcast_status: Option<String>,
    /// Maker cancel hints required for Coinset-primary cancel without an offer file.
    pub cancel_fields: OfferCancelFields,
}

#[derive(Debug, Clone)]
pub(crate) struct OfferArtifacts {
    pub offer: String,
    pub spend_bundle_hex: String,
    pub offer_nonce: String,
    pub selected_coin_ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PresplitArtifacts {
    pub split_spend_bundle_hex: Option<String>,
    pub presplit_coin_id: Option<String>,
    pub split_broadcast_status: Option<String>,
}

impl CreateOfferResult {
    pub(crate) fn assembled(
        execution_mode: OfferExecutionMode,
        core: OfferArtifacts,
        presplit: PresplitArtifacts,
        cancel_fields: OfferCancelFields,
    ) -> Self {
        Self {
            execution_mode,
            offer: core.offer,
            spend_bundle_hex: core.spend_bundle_hex,
            offer_nonce: core.offer_nonce,
            selected_coin_ids: core.selected_coin_ids,
            split_spend_bundle_hex: presplit.split_spend_bundle_hex,
            presplit_coin_id: presplit.presplit_coin_id,
            split_broadcast_status: presplit.split_broadcast_status,
            cancel_fields,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_protocol::Bytes32;

    fn sample_coin_id(byte: u8) -> Bytes32 {
        Bytes32::new([byte; 32])
    }

    fn base_request() -> CreateOfferRequest {
        CreateOfferRequest {
            receive_address: "xch1addr".to_string(),
            offer_asset_id: "cat".to_string(),
            offer_amount: 1000,
            request_asset_id: "xch".to_string(),
            request_amount: 500,
            offer_coin_ids: vec![sample_coin_id(0x01)],
            presplit_coin_ids: Vec::new(),
            split_input_coins: false,
            broadcast_split: false,
            expires_at: Some(1_700_000_000),
            bake_expiry_into_conditions: true,
            offer_nonce: None,
        }
    }

    #[test]
    fn offer_input_try_from_direct_and_presplit_paths() {
        let direct = OfferInput::try_from(base_request()).expect("direct");
        assert!(matches!(direct, OfferInput::Direct { .. }));

        let mut presplit_new = base_request();
        presplit_new.split_input_coins = true;
        presplit_new.broadcast_split = true;
        let presplit_new = OfferInput::try_from(presplit_new).expect("presplit new");
        assert!(matches!(presplit_new, OfferInput::PresplitNew { .. }));

        let mut presplit_existing = base_request();
        presplit_existing.presplit_coin_ids = vec![sample_coin_id(0x02)];
        let presplit_existing = OfferInput::try_from(presplit_existing).expect("presplit existing");
        assert!(matches!(
            presplit_existing,
            OfferInput::PresplitExisting { .. }
        ));

        let mut invalid = base_request();
        invalid.presplit_coin_ids = vec![sample_coin_id(0x02), sample_coin_id(0x03)];
        assert!(matches!(
            OfferInput::try_from(invalid),
            Err(SignerError::PresplitOfferRequiresSingleCoin)
        ));
    }

    #[test]
    fn offer_execution_mode_parse_db_and_display() {
        assert_eq!(
            OfferExecutionMode::parse_db("presplit_new"),
            Some(OfferExecutionMode::PresplitNew)
        );
        assert_eq!(OfferExecutionMode::PresplitNew.to_string(), "presplit_new");
        assert!(OfferExecutionMode::parse_db("unknown").is_none());
    }

    #[test]
    fn from_direct_build_omits_fixed_delegated_hash() {
        let fields = OfferCancelFields::from_direct_build("aa".repeat(32), "bb".repeat(32));
        assert_eq!(fields.input_coin_id.as_deref(), Some(&*"aa".repeat(32)));
        assert_eq!(fields.maker_puzzle_hash.as_deref(), Some(&*"bb".repeat(32)));
        assert!(fields.fixed_delegated_puzzle_hash.is_none());
    }

    #[test]
    fn create_offer_result_assembled_carries_presplit_fields() {
        let cancel = OfferCancelFields::from_presplit_build(
            "coin".to_string(),
            "delegated".to_string(),
            "makerp2".to_string(),
        );
        let result = CreateOfferResult::assembled(
            OfferExecutionMode::PresplitExisting,
            OfferArtifacts {
                offer: "offer1".to_string(),
                spend_bundle_hex: "ff".to_string(),
                offer_nonce: "nonce".to_string(),
                selected_coin_ids: vec!["aa".repeat(64)],
            },
            PresplitArtifacts {
                split_spend_bundle_hex: Some("bb".to_string()),
                presplit_coin_id: Some("cc".repeat(64)),
                split_broadcast_status: Some("submitted".to_string()),
            },
            cancel.clone(),
        );
        assert_eq!(result.execution_mode, OfferExecutionMode::PresplitExisting);
        assert_eq!(result.cancel_fields, cancel);
    }

    #[test]
    fn create_offer_request_deserializes_coin_ids_from_hex_strings() {
        let coin = "a".repeat(64);
        let raw = format!(
            r#"{{
                "receive_address": "xch1",
                "offer_asset_id": "cat",
                "offer_amount": 1,
                "request_asset_id": "xch",
                "request_amount": 1,
                "offer_coin_ids": ["{coin}"],
                "split_input_coins": false,
                "broadcast_split": false
            }}"#
        );
        let request: CreateOfferRequest = serde_json::from_str(&raw).expect("request");
        assert_eq!(request.offer_coin_ids.len(), 1);
        assert_eq!(hex::encode(request.offer_coin_ids[0]), coin);
    }

    #[test]
    fn conditions_expires_at_respects_soft_bake_flag() {
        let soft = OfferTerms {
            receive_address: "xch1".into(),
            offer_asset_id: "a".into(),
            offer_amount: 1,
            request_asset_id: "xch".into(),
            request_amount: 1,
            expires_at: Some(9),
            bake_expiry_into_conditions: false,
        };
        assert_eq!(soft.conditions_expires_at(), None);
        assert_eq!(
            OfferTerms {
                bake_expiry_into_conditions: true,
                ..soft
            }
            .conditions_expires_at(),
            Some(9)
        );
    }

    #[test]
    fn create_offer_request_deserializes_optional_offer_nonce() {
        let nonce = "b".repeat(64);
        let raw = format!(
            r#"{{
                "receive_address": "xch1",
                "offer_asset_id": "cat",
                "offer_amount": 1,
                "request_asset_id": "xch",
                "request_amount": 1,
                "split_input_coins": false,
                "broadcast_split": false,
                "offer_nonce": "{nonce}",
                "bake_expiry_into_conditions": false
            }}"#
        );
        let request: CreateOfferRequest = serde_json::from_str(&raw).expect("request");
        assert_eq!(
            request.offer_nonce.map(hex::encode).as_deref(),
            Some(nonce.as_str())
        );
        assert!(!request.bake_expiry_into_conditions);
        let empty = r#"{
                "receive_address": "xch1",
                "offer_asset_id": "cat",
                "offer_amount": 1,
                "request_asset_id": "xch",
                "request_amount": 1,
                "split_input_coins": false,
                "broadcast_split": false,
                "offer_nonce": ""
            }"#;
        let cleared: CreateOfferRequest = serde_json::from_str(empty).expect("empty nonce");
        assert!(cleared.offer_nonce.is_none());

        let with_nonce = CreateOfferRequest {
            offer_nonce: Some(Bytes32::new([0xab; 32])),
            ..base_request()
        };
        let encoded = serde_json::to_value(&with_nonce).expect("serialize some");
        assert_eq!(
            encoded.get("offer_nonce").and_then(|v| v.as_str()),
            Some(hex::encode([0xab; 32]).as_str())
        );
        let without = CreateOfferRequest {
            offer_nonce: None,
            ..base_request()
        };
        let encoded_none = serde_json::to_value(&without).expect("serialize none");
        assert!(encoded_none
            .get("offer_nonce")
            .is_none_or(serde_json::Value::is_null));
    }
}
