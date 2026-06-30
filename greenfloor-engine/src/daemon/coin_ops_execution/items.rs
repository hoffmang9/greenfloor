use serde_json::Value;

use crate::coin_ops::CoinOpPlan;
use crate::error::SignerResult;

use super::COIN_OP_ERROR_PREFIX;

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

pub(crate) type PlanSkip = (Vec<CoinOpExecItem>, u64);
pub(crate) type CoinOpSkipResult<T> = Result<T, PlanSkip>;

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

pub(crate) fn skip_item_for_plan(plan: &CoinOpPlan, reason: impl Into<String>) -> CoinOpExecItem {
    skip_item(
        plan.op_type.as_str(),
        plan.size_base_units,
        plan.op_count,
        reason,
    )
}

pub(crate) fn plan_skip(plan: &CoinOpPlan, reason: impl Into<String>) -> PlanSkip {
    (vec![skip_item_for_plan(plan, reason)], 0)
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

pub(crate) fn executed_item_for_plan(
    plan: &CoinOpPlan,
    reason: impl Into<String>,
    operation_id: String,
) -> CoinOpExecItem {
    executed_item(
        plan.op_type.as_str(),
        plan.size_base_units,
        plan.op_count,
        reason,
        operation_id,
    )
}

pub(crate) fn skip_on_signer_err_for_plan<T>(
    plan: &CoinOpPlan,
    result: SignerResult<T>,
) -> CoinOpSkipResult<T> {
    result.map_err(|err| {
        (
            vec![skip_item_for_plan(
                plan,
                format!("{COIN_OP_ERROR_PREFIX}:{err}"),
            )],
            0,
        )
    })
}

pub(crate) async fn execute_daemon_coin_op_plan(
    inner: impl std::future::Future<Output = CoinOpSkipResult<(Vec<CoinOpExecItem>, u64)>>,
) -> (Vec<CoinOpExecItem>, u64) {
    match inner.await {
        Ok(result) => result,
        Err(skip) => skip,
    }
}
