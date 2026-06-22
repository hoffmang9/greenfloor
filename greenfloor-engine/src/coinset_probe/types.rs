use std::future::Future;

use serde::Serialize;
use serde_json::Value;

use crate::error::SignerResult;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProbeAttempt {
    pub supported: bool,
    pub error: Option<String>,
    pub count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EndpointCapability {
    pub all_supported: bool,
    pub all_error: Option<String>,
    pub all_count: Option<usize>,
    pub range_supported: bool,
    pub range_error: Option<String>,
    pub range_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NamesCapability {
    pub sample_name: Option<String>,
    pub all_supported: Option<bool>,
    pub all_error: Option<String>,
    pub all_count: Option<usize>,
    pub range_supported: Option<bool>,
    pub range_error: Option<String>,
    pub range_count: Option<usize>,
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
    pub get_coin_records_by_puzzle_hashes: EndpointCapability,
    pub get_coin_records_by_hints: EndpointCapability,
    pub get_coin_records_by_names: NamesCapability,
}

impl ProbeAttempt {
    pub async fn run<F, Fut>(fetch: F) -> (Self, Option<Vec<Value>>)
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = SignerResult<Vec<Value>>>,
    {
        match fetch().await {
            Ok(records) => {
                let count = records.len();
                (
                    Self {
                        supported: true,
                        error: None,
                        count: Some(count),
                    },
                    Some(records),
                )
            }
            Err(err) => (
                Self {
                    supported: false,
                    error: Some(err.to_string()),
                    count: None,
                },
                None,
            ),
        }
    }
}

impl EndpointCapability {
    #[must_use]
    pub fn from_attempts(all: ProbeAttempt, range: ProbeAttempt) -> Self {
        Self {
            all_supported: all.supported,
            all_error: all.error,
            all_count: all.count,
            range_supported: range.supported,
            range_error: range.error,
            range_count: range.count,
        }
    }
}

impl NamesCapability {
    #[must_use]
    pub fn skipped() -> Self {
        Self {
            sample_name: None,
            all_supported: None,
            all_error: None,
            all_count: None,
            range_supported: None,
            range_error: None,
            range_count: None,
        }
    }

    #[must_use]
    pub fn invalid_sample(sample_name: &str, message: &str) -> Self {
        Self {
            sample_name: Some(sample_name.to_string()),
            all_supported: Some(false),
            all_error: Some(message.to_string()),
            all_count: None,
            range_supported: Some(false),
            range_error: Some(message.to_string()),
            range_count: None,
        }
    }

    #[must_use]
    pub fn from_endpoint(sample_name: String, endpoint: EndpointCapability) -> Self {
        Self {
            sample_name: Some(sample_name),
            all_supported: Some(endpoint.all_supported),
            all_error: endpoint.all_error,
            all_count: endpoint.all_count,
            range_supported: Some(endpoint.range_supported),
            range_error: endpoint.range_error,
            range_count: endpoint.range_count,
        }
    }
}

#[must_use]
pub fn sample_coin_id_from_records(records: &[Value]) -> Option<String> {
    use crate::coinset::coin_id_from_record;

    for record in records {
        let coin_id = coin_id_from_record(record);
        if !coin_id.is_empty() {
            return Some(coin_id);
        }
    }
    None
}

#[must_use]
pub fn scan_window_from_peak(peak_height: u64, height_window: u64) -> ScanWindow {
    let height_window = height_window.max(1);
    let start_height = peak_height.saturating_sub(height_window);
    ScanWindow {
        start_height,
        end_height: peak_height,
        peak_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_window_from_peak_applies_height_window() {
        let window = scan_window_from_peak(12_345, 50_000);
        assert_eq!(window.peak_height, 12_345);
        assert_eq!(window.end_height, 12_345);
        assert_eq!(window.start_height, 0);
    }

    #[test]
    fn scan_window_from_peak_subtracts_window_from_peak() {
        let window = scan_window_from_peak(100_000, 10_000);
        assert_eq!(window.start_height, 90_000);
        assert_eq!(window.end_height, 100_000);
    }

    #[test]
    fn sample_coin_id_from_records_prefers_first_resolvable_record() {
        let records = vec![
            serde_json::json!({"coin": {"amount": 1}}),
            serde_json::json!({
                "coin": {
                    "parent_coin_info": format!("0x{}", "a".repeat(64)),
                    "puzzle_hash": format!("0x{}", "b".repeat(64)),
                    "amount": 2
                }
            }),
        ];
        let sample = sample_coin_id_from_records(&records).expect("sample");
        assert_eq!(sample.len(), 64);
    }

    #[test]
    fn endpoint_capability_from_attempts_maps_fields() {
        let capability = EndpointCapability::from_attempts(
            ProbeAttempt {
                supported: true,
                error: None,
                count: Some(3),
            },
            ProbeAttempt {
                supported: false,
                error: Some("range failed".to_string()),
                count: None,
            },
        );
        assert!(capability.all_supported);
        assert_eq!(capability.all_count, Some(3));
        assert!(!capability.range_supported);
        assert_eq!(capability.range_error.as_deref(), Some("range failed"));
    }

    #[test]
    fn names_capability_skipped_serializes_null_fields() {
        let payload = serde_json::to_value(NamesCapability::skipped()).expect("json");
        assert!(payload.get("sample_name").unwrap().is_null());
        assert!(payload.get("all_supported").unwrap().is_null());
    }

    #[tokio::test]
    async fn probe_attempt_run_maps_success_and_failure() {
        use crate::error::SignerError;

        let (ok, records) =
            ProbeAttempt::run(|| async { Ok(vec![serde_json::json!({"coin": {"amount": 1}})]) })
                .await;
        assert!(ok.supported);
        assert_eq!(ok.count, Some(1));
        assert_eq!(records.as_ref().map(Vec::len), Some(1));

        let (err, records) = ProbeAttempt::run(|| async {
            Err::<Vec<serde_json::Value>, _>(SignerError::Other("boom".to_string()))
        })
        .await;
        assert!(!err.supported);
        assert_eq!(err.error.as_deref(), Some("boom"));
        assert!(records.is_none());
    }
}
