use std::future::Future;

use serde_json::Value;

use super::super::json_util::to_coinset_hex;
use super::super::parse::coin_id_from_record;
use super::super::scan_client::DirectCoinsetScanClient;
use super::types::{HeightWindowCapability, ProbedHeightWindowCapability, ScanWindow};
use crate::error::SignerResult;
use crate::hex::hex_to_bytes32;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeAttempt {
    supported: bool,
    error: Option<String>,
    count: Option<usize>,
}

impl ProbeAttempt {
    async fn run<F, Fut>(fetch: F) -> (Self, Option<Vec<Value>>)
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

fn probed_from_attempts(
    all: ProbeAttempt,
    range: ProbeAttempt,
    sample_name: Option<String>,
) -> HeightWindowCapability {
    HeightWindowCapability::Probed(ProbedHeightWindowCapability {
        sample_name,
        all_supported: all.supported,
        all_error: all.error,
        all_count: all.count,
        range_supported: range.supported,
        range_error: range.error,
        range_count: range.count,
    })
}

pub async fn probe_height_window<F, Fut>(
    start_height: u64,
    end_height: u64,
    sample_name: Option<String>,
    fetch: F,
) -> (HeightWindowCapability, Option<Vec<Value>>)
where
    F: Fn(Option<u64>, Option<u64>) -> Fut,
    Fut: Future<Output = SignerResult<Vec<Value>>>,
{
    let (all, records) = ProbeAttempt::run(|| fetch(None, None)).await;
    let (range, _) = ProbeAttempt::run(|| fetch(Some(start_height), Some(end_height))).await;
    (probed_from_attempts(all, range, sample_name), records)
}

pub async fn probe_names(
    client: &DirectCoinsetScanClient,
    sample_name: Option<&str>,
    start_height: u64,
    end_height: u64,
) -> HeightWindowCapability {
    let Some(sample_name) = sample_name.filter(|value| !value.is_empty()) else {
        return HeightWindowCapability::skipped();
    };
    let Ok(sample_bytes) = hex_to_bytes32(sample_name) else {
        return HeightWindowCapability::invalid_sample(sample_name, "invalid sample coin id hex");
    };
    let names = vec![to_coinset_hex(sample_bytes.as_ref())];
    let (capability, _) = probe_height_window(
        start_height,
        end_height,
        Some(sample_name.to_string()),
        |start, end| client.by_names(&names, true, start, end),
    )
    .await;
    capability
}

#[must_use]
pub fn sample_coin_id_from_records(records: &[Value]) -> Option<String> {
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
    use crate::error::SignerError;

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
    fn height_window_capability_from_attempts_maps_fields() {
        let capability = probed_from_attempts(
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
            None,
        );
        assert!(capability.all_supported());
        assert_eq!(capability.all_count(), Some(3));
        assert!(!capability.range_supported());
        assert_eq!(capability.range_error(), Some("range failed"));
    }

    #[test]
    fn height_window_capability_skipped_serializes_null_fields() {
        let payload = serde_json::to_value(HeightWindowCapability::skipped()).expect("json");
        assert!(payload.get("sample_name").unwrap().is_null());
        assert!(payload.get("all_supported").unwrap().is_null());
    }

    #[tokio::test]
    async fn probe_attempt_run_maps_success_and_failure() {
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
