use serde_json::Value;

use crate::error::SignerResult;

#[derive(Debug, Clone)]
pub struct CoinOpExecItem {
    pub op_type: String,
    pub size_base_units: i64,
    pub op_count: i64,
    pub status: String,
    pub reason: String,
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CoinOpExecutionResult {
    pub dry_run: bool,
    pub planned_count: usize,
    pub executed_count: u64,
    pub status: String,
    pub items: Vec<CoinOpExecItem>,
    pub signer_selection: Value,
}

pub(crate) fn skip_item(
    op_type: &str,
    size_base_units: i64,
    op_count: i64,
    reason: impl Into<String>,
) -> CoinOpExecItem {
    CoinOpExecItem {
        op_type: op_type.to_string(),
        size_base_units,
        op_count,
        status: "skipped".to_string(),
        reason: reason.into(),
        operation_id: None,
    }
}

pub(crate) fn executed_item(
    op_type: &str,
    size_base_units: i64,
    op_count: i64,
    reason: impl Into<String>,
    operation_id: String,
) -> CoinOpExecItem {
    CoinOpExecItem {
        op_type: op_type.to_string(),
        size_base_units,
        op_count,
        status: "executed".to_string(),
        reason: reason.into(),
        operation_id: Some(operation_id),
    }
}

pub(crate) fn skip_on_signer_err<T>(
    op_type: &str,
    size_base_units: i64,
    op_count: i64,
    result: SignerResult<T>,
) -> Result<T, (Vec<CoinOpExecItem>, u64)> {
    result.map_err(|err| {
        (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                format!("{}:{err}", super::COIN_OP_ERROR_PREFIX),
            )],
            0,
        )
    })
}
