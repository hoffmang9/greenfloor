use crate::error::SignerResult;
use crate::vault_coinset_scan::request::ScanRequest;
use crate::vault_coinset_scan::result::ScanResult;
use crate::vault_coinset_scan::state::ScanState;

pub async fn run_vault_coinset_scan(request: ScanRequest) -> SignerResult<ScanResult> {
    ScanState::run(request).await
}
