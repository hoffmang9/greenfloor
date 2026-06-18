//! Structured CLI exit payloads for coin-op iteration (emitted at command boundary).

use serde_json::Value;

#[derive(Debug, Clone)]
pub(super) struct CoinOpCliExit {
    pub code: i32,
    pub payload: Value,
}
