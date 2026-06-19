use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    PuzzleHash,
    Hint,
}

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
    #[serde(rename = "type", alias = "coin_type")]
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coin_row_serializes_type_field() {
        let row = CoinRow {
            coin_id: "a".repeat(64),
            puzzle_hash: "b".repeat(64),
            parent_coin_info: "c".repeat(64),
            amount: 1000,
            confirmed_block_index: 1,
            spent_block_index: 0,
            discovered_nonces: vec![0],
            discovered_by_puzzle_hash: true,
            discovered_by_hint: false,
            kind: CoinKind::Cat,
            cat_asset_id: Some("d".repeat(64)),
            cat_symbols: vec!["wusdc".to_string()],
        };
        let value = serde_json::to_value(&row).expect("serialize coin row");
        assert_eq!(value.get("type").and_then(|v| v.as_str()), Some("CAT"));
        assert!(value.get("coin_type").is_none());
    }

    #[test]
    fn coin_row_deserializes_legacy_coin_type_alias() {
        let row: CoinRow = serde_json::from_value(json!({
            "coin_id": "a".repeat(64),
            "puzzle_hash": "b".repeat(64),
            "parent_coin_info": "c".repeat(64),
            "amount": 1000,
            "confirmed_block_index": 1,
            "spent_block_index": 0,
            "discovered_nonces": [0],
            "discovered_by_puzzle_hash": true,
            "discovered_by_hint": false,
            "coin_type": "XCH",
            "cat_symbols": [],
        }))
        .expect("deserialize legacy coin row");
        assert_eq!(row.kind, CoinKind::Xch);
    }
}
