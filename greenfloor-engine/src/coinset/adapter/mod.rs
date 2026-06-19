//! Script-facing Coinset client for direct API hosts (`api.coinset.org`).

mod client;
mod network;
mod parse;

#[cfg(test)]
mod tests;

pub use client::CoinsetReadClient;
pub use network::{
    normalize_coinset_network, resolve_coinset_base_url, MAINNET_BASE_URL, TESTNET11_BASE_URL,
};
