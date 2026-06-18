use chia_protocol::Bytes32;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{SignerError, SignerResult};

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
}

/// Typed offer path after parsing [`CreateOfferRequest`].
///
/// # Execution vs input mode
///
/// [`OfferInput::PresplitNew`] means the operator enabled `--split-input-coins`.
/// Planning still selects the direct offer path when selected CAT inputs already
/// equal `--offer-amount` exactly (no change to split off). In that case execution
/// uses the direct offer assembler and `execution_mode` is
/// [`OfferExecutionMode::Direct`], even though the input variant is `PresplitNew`.
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
        };

        if !request.presplit_coin_ids.is_empty() {
            if request.presplit_coin_ids.len() != 1 {
                return Err(SignerError::PresplitOfferRequiresSingleCoin);
            }
            return Ok(Self::PresplitExisting {
                terms,
                presplit_coin_id: request.presplit_coin_ids[0],
                source_coin_ids: request.offer_coin_ids,
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
        .map(|value| {
            crate::vault::members::hex_to_bytes32(&value).map_err(serde::de::Error::custom)
        })
        .collect()
}

pub(crate) fn deserialize_bytes32<'de, D>(deserializer: D) -> Result<Bytes32, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    crate::vault::members::hex_to_bytes32(&raw).map_err(serde::de::Error::custom)
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
        }
    }
}
