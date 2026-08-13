use thiserror::Error;

mod coin_ops;
mod config;
mod offer;
mod persistence;
mod transport;
mod vault;

pub use coin_ops::CoinOpsError;
pub use config::ConfigError;
pub use offer::OfferError;
pub use persistence::PersistenceError;
pub use transport::TransportError;
pub use vault::VaultError;

/// Operator failure with a single domain owner per variant.
#[derive(Debug, Error)]
pub enum SignerError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    CoinOps(#[from] CoinOpsError),
    #[error(transparent)]
    Offer(#[from] OfferError),
    #[error(transparent)]
    Reconcile(#[from] crate::cycle::ReconcileStateError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("{0}")]
    Other(String),
}

const MIXED_SPLIT_SELECTED_COINS_NOT_SPENDABLE: &str = "Some selected coins are not spendable";

impl SignerError {
    /// Coinset RPC application failure (`success: false`). HTTP/transport uses [`Self::from_reqwest`].
    #[must_use]
    pub fn coinset(message: impl Into<String>) -> Self {
        Self::Transport(TransportError::Coinset(message.into()))
    }

    /// Generic HTTP transport failure (`layer` names the client, e.g. `http` or `dexie`).
    #[must_use]
    pub fn http(layer: &'static str, message: impl Into<String>) -> Self {
        Self::Transport(TransportError::Http {
            layer,
            message: message.into(),
        })
    }

    /// HTTP timeout (`reqwest::Error::is_timeout`).
    #[must_use]
    pub fn http_timeout(layer: &'static str, message: impl Into<String>) -> Self {
        Self::Transport(TransportError::Timeout {
            layer,
            message: message.into(),
        })
    }

    /// HTTP connect failure (`reqwest::Error::is_connect`).
    #[must_use]
    pub fn http_connect(layer: &'static str, message: impl Into<String>) -> Self {
        Self::Transport(TransportError::Connect {
            layer,
            message: message.into(),
        })
    }

    /// HTTP decode failure (`reqwest::Error::is_decode`).
    #[must_use]
    pub fn http_decode(layer: &'static str, message: impl Into<String>) -> Self {
        Self::Transport(TransportError::Decode {
            layer,
            message: message.into(),
        })
    }

    /// HTTP request/body failure (`reqwest::Error::is_request` / `is_body`).
    #[must_use]
    pub fn http_request(layer: &'static str, message: impl Into<String>) -> Self {
        Self::Transport(TransportError::Request {
            layer,
            message: message.into(),
        })
    }

    #[must_use]
    pub fn from_reqwest(layer: &'static str, err: &reqwest::Error) -> Self {
        Self::Transport(TransportError::from_reqwest(layer, err))
    }

    /// HTTP response with a non-success status code.
    #[must_use]
    pub fn http_status(layer: &'static str, status: u16, message: impl Into<String>) -> Self {
        Self::Transport(TransportError::HttpStatus {
            layer,
            status,
            message: message.into(),
        })
    }

    #[must_use]
    pub fn is_http_not_found(&self) -> bool {
        matches!(self, Self::Transport(err) if err.is_http_not_found())
    }

    /// chia-wallet-sdk driver failure.
    #[must_use]
    pub fn driver(message: impl Into<String>) -> Self {
        Self::Transport(TransportError::Driver(message.into()))
    }

    #[must_use]
    pub fn is_mixed_split_selected_coins_not_spendable(&self) -> bool {
        matches!(
            self,
            Self::Vault(VaultError::MixedSplitSelectedCoinsNotSpendable)
        )
    }

    #[must_use]
    pub fn is_sqlite_fatal(&self) -> bool {
        match self {
            Self::Persistence(err) => err.is_sqlite_fatal(),
            _ => false,
        }
    }

    #[must_use]
    pub fn is_retryable_upstream(&self) -> bool {
        matches!(self, Self::Transport(err) if err.is_retryable_upstream())
    }

    #[must_use]
    pub fn is_parallel_dispatch_transient(&self) -> bool {
        if self.is_sqlite_fatal() {
            return false;
        }
        match self {
            Self::Persistence(err) => err.is_parallel_dispatch_transient(),
            Self::Transport(err) => err.is_retryable_upstream(),
            _ => false,
        }
    }
}

pub type SignerResult<T> = Result<T, SignerError>;

#[must_use]
pub fn driver_error(err: &chia_sdk_driver::DriverError) -> SignerError {
    if let chia_sdk_driver::DriverError::Custom(message) = err {
        if message.contains(MIXED_SPLIT_SELECTED_COINS_NOT_SPENDABLE) {
            return VaultError::MixedSplitSelectedCoinsNotSpendable.into();
        }
    }
    SignerError::driver(err.to_string())
}

impl From<chia_sdk_driver::DriverError> for SignerError {
    fn from(err: chia_sdk_driver::DriverError) -> Self {
        driver_error(&err)
    }
}

impl From<reqwest::Error> for SignerError {
    fn from(err: reqwest::Error) -> Self {
        SignerError::from_reqwest("http", &err)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CoinOpsError, ConfigError, OfferError, PersistenceError, SignerError, TransportError,
        VaultError,
    };

    #[test]
    fn signer_error_display_messages_are_stable() {
        let cases: Vec<(SignerError, &str)> = vec![
            (
                VaultError::LauncherIdInvalid.into(),
                "vault launcher id missing or invalid",
            ),
            (
                CoinOpsError::InsufficientCatCoins.into(),
                "insufficient cat coins",
            ),
            (
                CoinOpsError::CatLineageResolutionFailed("abcd".to_string()).into(),
                "failed to resolve cat lineage for coin abcd",
            ),
            (
                OfferError::OfferInputRequiresPresplit.into(),
                "offer input exceeds offer amount; enable split-input-coins or specify exact coin",
            ),
            (
                OfferError::PresplitCoinConfirmationTimeout.into(),
                "timeout waiting for presplit coin confirmation",
            ),
            (
                VaultError::KmsPublicKeyMismatch {
                    kms: "aa".to_string(),
                    custody: "bb".to_string(),
                }
                .into(),
                "kms public key mismatch: kms=aa custody=bb",
            ),
            (
                ConfigError::MissingField("signer").into(),
                "missing config field: signer",
            ),
            (
                ConfigError::Parse("markets config root must be a mapping".to_string()).into(),
                "markets config root must be a mapping",
            ),
            (
                OfferError::ResolvedAssetsCollideForNonXchPair.into(),
                "signer_asset_resolution_failed:resolved_assets_collide_for_non_xch_pair",
            ),
            (SignerError::coinset("down"), "coinset error: down"),
            (
                SignerError::http("dexie", "bad json"),
                "http error (dexie): bad json",
            ),
            (
                SignerError::http_timeout("dexie", "timed out"),
                "http timeout (dexie): timed out",
            ),
            (
                SignerError::http_connect("dexie", "connection refused"),
                "http connect (dexie): connection refused",
            ),
            (
                SignerError::http_decode("coinset", "error decoding response body"),
                "http decode (coinset): error decoding response body",
            ),
            (
                SignerError::http_request("coinset", "error sending request"),
                "http request (coinset): error sending request",
            ),
            (
                SignerError::http_status("dexie_http_error", 404, "missing"),
                "http status 404 (dexie_http_error): missing",
            ),
            (
                SignerError::driver("invalid mod hash"),
                "driver error: invalid mod hash",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn transient_http_uses_timeout_and_connect_variants() {
        assert!(
            SignerError::http_timeout("dexie_network_error", "timed out")
                .is_parallel_dispatch_transient()
        );
        assert!(
            SignerError::http_connect("dexie_network_error", "connection refused")
                .is_parallel_dispatch_transient()
        );
        assert!(!SignerError::http("dexie", "timeout").is_parallel_dispatch_transient());
        assert!(
            !SignerError::http_status("dexie_http_error", 404, "missing")
                .is_parallel_dispatch_transient()
        );
        assert!(SignerError::http_status("dexie_http_error", 404, "missing").is_http_not_found());
        assert!(!SignerError::http("dexie", "missing").is_http_not_found());
        assert!(
            !SignerError::Other("connection reset".to_string()).is_parallel_dispatch_transient()
        );
        assert!(!SignerError::Other("invalid offer".to_string()).is_parallel_dispatch_transient());
    }

    #[test]
    fn parallel_dispatch_transient_matches_typed_contention_and_upstream() {
        assert!(SignerError::http_timeout("dexie", "timed out").is_parallel_dispatch_transient());
        let contention: SignerError =
            PersistenceError::ReservationContention("busy".to_string()).into();
        assert!(contention.is_parallel_dispatch_transient());
        assert!(
            !SignerError::Other("PermanentOfferBuildFailure: bad puzzle".to_string())
                .is_parallel_dispatch_transient()
        );
    }

    #[test]
    fn sqlite_fatal_errors_are_not_parallel_dispatch_transient() {
        assert!(
            SignerError::Persistence(PersistenceError::SqliteOpenFailed {
                path: "/tmp/greenfloor.sqlite".to_string(),
                open_error: "unable to open database file".to_string(),
            })
            .is_sqlite_fatal()
        );
        assert!(!SignerError::Other("database is locked".to_string()).is_sqlite_fatal());
        assert!(
            !SignerError::Persistence(PersistenceError::SqliteOpenFailed {
                path: "/tmp/x".to_string(),
                open_error: "permission denied".to_string(),
            })
            .is_parallel_dispatch_transient()
        );
        assert!(SignerError::Persistence(PersistenceError::DatabaseLocked)
            .is_parallel_dispatch_transient());
    }

    #[test]
    fn parallel_dispatch_transient_classifies_coinset_and_http_status() {
        assert!(!SignerError::driver("invalid mod hash").is_parallel_dispatch_transient());
        assert!(!SignerError::http("dexie", "bad json").is_parallel_dispatch_transient());
        assert!(!SignerError::CoinOps(CoinOpsError::InsufficientCatCoins)
            .is_parallel_dispatch_transient());
        assert!(!SignerError::coinset("connection refused").is_retryable_upstream());
        assert!(!SignerError::coinset("invalid puzzle hash").is_retryable_upstream());
        assert!(SignerError::http_connect("coinset", "connection refused").is_retryable_upstream());
        assert!(
            SignerError::http_decode("coinset", "error decoding response body")
                .is_retryable_upstream()
        );
        assert!(
            SignerError::http_request("coinset", "error sending request").is_retryable_upstream()
        );
        assert!(
            SignerError::http_status("dexie_http_error", 503, "unavailable")
                .is_retryable_upstream()
        );
        assert!(
            !SignerError::http_status("dexie_http_error", 400, "Invalid Offer")
                .is_retryable_upstream()
        );
        assert!(!SignerError::Other(
            "parse body json: expected value at line 1 column 1".to_string()
        )
        .is_retryable_upstream());
    }

    #[test]
    fn mixed_split_selected_coins_not_spendable_is_classified() {
        assert!(
            SignerError::Vault(VaultError::MixedSplitSelectedCoinsNotSpendable)
                .is_mixed_split_selected_coins_not_spendable()
        );
        assert!(
            !SignerError::Other("upstream: Some selected coins are not spendable".to_string())
                .is_mixed_split_selected_coins_not_spendable()
        );
    }

    #[test]
    fn driver_error_maps_chia_driver_failures() {
        use super::driver_error;
        use chia_sdk_driver::DriverError;

        let mapped = driver_error(&DriverError::InvalidModHash);
        assert!(matches!(
            mapped,
            SignerError::Transport(TransportError::Driver(_))
        ));
        assert!(mapped.to_string().contains("invalid mod hash"));

        let from_impl: SignerError = DriverError::InvalidModHash.into();
        assert_eq!(from_impl.to_string(), mapped.to_string());

        let unspendable = driver_error(&DriverError::Custom(
            "Some selected coins are not spendable".to_string(),
        ));
        assert!(matches!(
            unspendable,
            SignerError::Vault(VaultError::MixedSplitSelectedCoinsNotSpendable)
        ));
    }
}
