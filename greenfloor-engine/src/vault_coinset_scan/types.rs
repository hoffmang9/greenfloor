use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinKind {
    Xch,
    Cat,
    Other,
    Unknown,
}

impl CoinKind {
    pub fn is_xch(self) -> bool {
        matches!(self, Self::Xch)
    }

    pub fn is_cat(self) -> bool {
        matches!(self, Self::Cat)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoinRow {
    pub coin_id: String,
    pub puzzle_hash: String,
    pub parent_coin_info: String,
    pub amount: u64,
    pub confirmed_block_index: u64,
    pub spent_block_index: u64,
    pub discovered_nonces: Vec<u32>,
    pub discovered_by_puzzle_hash: bool,
    pub discovered_by_hint: bool,
    #[serde(rename = "coin_type", alias = "type")]
    pub kind: CoinKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cat_asset_id: Option<String>,
    pub cat_symbols: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetTypeFilter {
    All,
    Xch,
    Cat,
}

impl AssetTypeFilter {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "xch" => Self::Xch,
            "cat" => Self::Cat,
            _ => Self::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStopReason {
    MaxNonceReached,
    EmptyNonceBatches,
    ScanWindowExhausted,
}
