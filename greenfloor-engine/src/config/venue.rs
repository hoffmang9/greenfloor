//! Offer publish / reconcile venue (`coinset` | `dexie` | `splash`).

use crate::error::{SignerError, SignerResult};

/// Canonical offer publish and lifecycle venue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Venue {
    Coinset,
    Dexie,
    Splash,
}

impl Venue {
    /// Parse a venue string (case-insensitive). Empty / unknown → error.
    ///
    /// # Errors
    ///
    /// Returns an error when `raw` is not one of `coinset`, `dexie`, or `splash`.
    pub fn parse(raw: &str) -> SignerResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "coinset" => Ok(Self::Coinset),
            "dexie" => Ok(Self::Dexie),
            "splash" => Ok(Self::Splash),
            other => Err(SignerError::Other(format!(
                "offer venue must be coinset, dexie, or splash (got {other})"
            ))),
        }
    }

    /// Parse a persisted / optional venue. `None` and blank → `None` (not an error).
    #[must_use]
    pub fn parse_optional(raw: Option<&str>) -> Option<Self> {
        let value = raw.map(str::trim).filter(|value| !value.is_empty())?;
        Self::parse(value).ok()
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Coinset => "coinset",
            Self::Dexie => "dexie",
            Self::Splash => "splash",
        }
    }

    #[must_use]
    pub const fn is_dexie(self) -> bool {
        matches!(self, Self::Dexie)
    }

    /// Dexie is authoritative only for an explicit persisted `dexie` venue.
    #[must_use]
    pub fn is_dexie_authoritative(publish_venue: Option<&str>) -> bool {
        Self::parse_optional(publish_venue).is_some_and(Self::is_dexie)
    }
}

impl std::fmt::Display for Venue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_known_venues_case_insensitive() {
        assert_eq!(Venue::parse("Coinset").expect("ok"), Venue::Coinset);
        assert_eq!(Venue::parse("DEXIE").expect("ok"), Venue::Dexie);
        assert_eq!(Venue::parse("splash").expect("ok"), Venue::Splash);
    }

    #[test]
    fn parse_rejects_unknown() {
        assert!(Venue::parse("webhook").is_err());
        assert!(Venue::parse("").is_err());
    }

    #[test]
    fn dexie_authoritative_is_explicit_only() {
        assert!(Venue::is_dexie_authoritative(Some("dexie")));
        assert!(!Venue::is_dexie_authoritative(Some("coinset")));
        assert!(!Venue::is_dexie_authoritative(Some("splash")));
        assert!(!Venue::is_dexie_authoritative(None));
        assert!(!Venue::is_dexie_authoritative(Some("")));
    }
}
