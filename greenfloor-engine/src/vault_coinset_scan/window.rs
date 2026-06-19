use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone, Copy)]
pub struct ScanWindowPlan {
    pub effective_start_height: Option<u64>,
    pub effective_end_height: Option<u64>,
    pub chain_peak_height: Option<u64>,
    pub checkpoint_synced_height: Option<u64>,
    pub exhausted: bool,
}

pub fn resolve_scan_window(
    requested_start_height: Option<u64>,
    requested_end_height: Option<u64>,
    incremental_from_checkpoint: bool,
    checkpoint_last_synced_height: Option<u64>,
    chain_peak_height: Option<u64>,
) -> SignerResult<ScanWindowPlan> {
    if let (Some(start), Some(end)) = (requested_start_height, requested_end_height) {
        if end < start {
            return Err(SignerError::Other(
                "end-height must be greater than or equal to start-height".to_string(),
            ));
        }
    }
    if incremental_from_checkpoint && requested_start_height.is_some() {
        return Err(SignerError::Other(
            "cannot use --start-height with --incremental-from-checkpoint".to_string(),
        ));
    }

    let mut effective_start_height = requested_start_height;
    if incremental_from_checkpoint {
        if let Some(last) = checkpoint_last_synced_height {
            effective_start_height = Some(last.saturating_add(1));
        } else if effective_start_height.is_none() {
            effective_start_height = Some(0);
        }
    }

    let mut effective_end_height = requested_end_height;
    if effective_end_height.is_none() {
        effective_end_height = chain_peak_height;
    }

    let checkpoint_synced_height = effective_end_height.or(chain_peak_height);
    let exhausted = matches!(
        (effective_start_height, effective_end_height),
        (Some(start), Some(end)) if start > end
    );

    Ok(ScanWindowPlan {
        effective_start_height,
        effective_end_height,
        chain_peak_height,
        checkpoint_synced_height,
        exhausted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_scan_starts_after_checkpoint_height() {
        let plan = resolve_scan_window(None, None, true, Some(99), Some(200)).expect("plan");
        assert_eq!(plan.effective_start_height, Some(100));
        assert_eq!(plan.effective_end_height, Some(200));
        assert!(!plan.exhausted);
    }

    #[test]
    fn scan_window_exhausted_when_incremental_start_past_end() {
        let plan = resolve_scan_window(None, Some(200), true, Some(300), Some(400)).expect("plan");
        assert!(plan.exhausted);
        assert_eq!(plan.effective_start_height, Some(301));
        assert_eq!(plan.effective_end_height, Some(200));
    }

    #[test]
    fn rejects_start_height_with_incremental_mode() {
        let err =
            resolve_scan_window(Some(1), None, true, Some(10), Some(20)).expect_err("conflict");
        assert!(err.to_string().contains("incremental-from-checkpoint"));
    }
}
