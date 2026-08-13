//! Shared `reqwest::Client` for Dexie/Splash so daemon cycles reuse connections.

use std::sync::LazyLock;

use reqwest::Client;

static SHARED_HTTP: LazyLock<Client> = LazyLock::new(Client::new);

/// Process-wide HTTP client for venue adapters.
#[must_use]
pub fn shared_http_client() -> Client {
    SHARED_HTTP.clone()
}
