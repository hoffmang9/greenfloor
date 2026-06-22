use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbedHeightWindowCapability {
    pub sample_name: Option<String>,
    pub all_supported: bool,
    pub all_error: Option<String>,
    pub all_count: Option<usize>,
    pub range_supported: bool,
    pub range_error: Option<String>,
    pub range_count: Option<usize>,
}

/// Height-window probe result for a Coinset records endpoint.
///
/// `Skipped` serializes all fields as JSON null (names probe when no sample coin exists).
/// `Probed` serializes concrete probe outcomes (puzzle hashes, hints, and probed names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeightWindowCapability {
    Skipped,
    Probed(ProbedHeightWindowCapability),
}

impl HeightWindowCapability {
    #[must_use]
    pub fn probed_counts(all_count: usize, range_count: usize) -> Self {
        Self::Probed(ProbedHeightWindowCapability {
            sample_name: None,
            all_supported: true,
            all_error: None,
            all_count: Some(all_count),
            range_supported: true,
            range_error: None,
            range_count: Some(range_count),
        })
    }

    #[must_use]
    pub fn skipped() -> Self {
        Self::Skipped
    }

    #[must_use]
    pub fn invalid_sample(sample_name: &str, message: &str) -> Self {
        Self::Probed(ProbedHeightWindowCapability {
            sample_name: Some(sample_name.to_string()),
            all_supported: false,
            all_error: Some(message.to_string()),
            all_count: None,
            range_supported: false,
            range_error: Some(message.to_string()),
            range_count: None,
        })
    }

    #[must_use]
    pub fn probed(&self) -> Option<&ProbedHeightWindowCapability> {
        match self {
            Self::Skipped => None,
            Self::Probed(probed) => Some(probed),
        }
    }

    #[must_use]
    pub fn all_supported(&self) -> bool {
        self.probed().is_some_and(|probed| probed.all_supported)
    }

    #[must_use]
    pub fn all_supported_opt(&self) -> Option<bool> {
        self.probed().map(|probed| probed.all_supported)
    }

    #[must_use]
    pub fn all_count(&self) -> Option<usize> {
        self.probed().and_then(|probed| probed.all_count)
    }

    #[must_use]
    pub fn range_supported(&self) -> bool {
        self.probed().is_some_and(|probed| probed.range_supported)
    }

    #[must_use]
    pub fn range_error(&self) -> Option<&str> {
        self.probed()
            .and_then(|probed| probed.range_error.as_deref())
    }

    #[must_use]
    pub fn sample_name(&self) -> Option<&str> {
        self.probed()
            .and_then(|probed| probed.sample_name.as_deref())
    }
}

impl Serialize for HeightWindowCapability {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(7))?;
        match self {
            Self::Skipped => {
                map.serialize_entry("sample_name", &None::<String>)?;
                map.serialize_entry("all_supported", &None::<bool>)?;
                map.serialize_entry("all_error", &None::<String>)?;
                map.serialize_entry("all_count", &None::<usize>)?;
                map.serialize_entry("range_supported", &None::<bool>)?;
                map.serialize_entry("range_error", &None::<String>)?;
                map.serialize_entry("range_count", &None::<usize>)?;
            }
            Self::Probed(probed) => {
                map.serialize_entry("sample_name", &probed.sample_name)?;
                map.serialize_entry("all_supported", &probed.all_supported)?;
                map.serialize_entry("all_error", &probed.all_error)?;
                map.serialize_entry("all_count", &probed.all_count)?;
                map.serialize_entry("range_supported", &probed.range_supported)?;
                map.serialize_entry("range_error", &probed.range_error)?;
                map.serialize_entry("range_count", &probed.range_count)?;
            }
        }
        map.end()
    }
}

#[derive(Debug, Serialize)]
pub struct ProbeReport {
    pub network: String,
    pub coinset_base_url: String,
    pub launcher_id: String,
    pub launcher_id_source: String,
    pub probe_nonce: u32,
    pub probe_p2_hash: String,
    pub scan_window: ScanWindow,
    pub capabilities: CapabilitiesReport,
}

#[derive(Debug, Serialize)]
pub struct ScanWindow {
    pub start_height: u64,
    pub end_height: u64,
    pub peak_height: u64,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_field_names)]
pub struct CapabilitiesReport {
    pub get_coin_records_by_puzzle_hashes: HeightWindowCapability,
    pub get_coin_records_by_hints: HeightWindowCapability,
    pub get_coin_records_by_names: HeightWindowCapability,
}
