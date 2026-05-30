//! Typed PyO3 request/response types shared by daemon and manager entrypoints.

use std::path::PathBuf;

use engine_core::daemon::{
    reconcile_offers_batch, DaemonCycleSummary, ReconcileBatchItem, ReconcileBatchResult,
};
use engine_core::manager::{build_and_post_offer, BuildAndPostOfferRequest, BuildAndPostOfferResponse};
use pyo3::prelude::*;
use pyo3::types::PyModule;

use crate::py_utils::{dict_from_json_value, to_py_err};
use crate::runtime;

#[pyclass(name = "DaemonCycleSummary")]
#[derive(Clone)]
pub(crate) struct PyDaemonCycleSummary {
    #[pyo3(get)]
    duration_ms: u64,
    #[pyo3(get)]
    enabled_markets: usize,
    #[pyo3(get)]
    markets_attempted: usize,
    #[pyo3(get)]
    markets_processed: u64,
    #[pyo3(get)]
    runtime_market_slot_count: u64,
    #[pyo3(get)]
    stale_open_sweep_checked_offer_count: u64,
    #[pyo3(get)]
    stale_open_sweep_requeue_market_ids: Vec<String>,
    #[pyo3(get)]
    stale_open_sweep_requeue_count: usize,
    #[pyo3(get)]
    stale_open_sweep_truncated: bool,
    #[pyo3(get)]
    immediate_requeue_market_ids: Vec<String>,
    #[pyo3(get)]
    immediate_requeue_count: usize,
    #[pyo3(get)]
    error_count: u64,
    #[pyo3(get)]
    strategy_planned_total: u64,
    #[pyo3(get)]
    strategy_executed_total: u64,
    #[pyo3(get)]
    cancel_triggered_count: u64,
    #[pyo3(get)]
    cancel_planned_total: u64,
    #[pyo3(get)]
    cancel_executed_total: u64,
    #[pyo3(get)]
    consumed_immediate_requeues: Vec<String>,
}

impl From<DaemonCycleSummary> for PyDaemonCycleSummary {
    fn from(value: DaemonCycleSummary) -> Self {
        Self {
            duration_ms: value.duration_ms,
            enabled_markets: value.enabled_markets,
            markets_attempted: value.markets_attempted,
            markets_processed: value.markets_processed,
            runtime_market_slot_count: value.runtime_market_slot_count,
            stale_open_sweep_checked_offer_count: value.stale_open_sweep_checked_offer_count,
            stale_open_sweep_requeue_market_ids: value.stale_open_sweep_requeue_market_ids,
            stale_open_sweep_requeue_count: value.stale_open_sweep_requeue_count,
            stale_open_sweep_truncated: value.stale_open_sweep_truncated,
            immediate_requeue_market_ids: value.immediate_requeue_market_ids,
            immediate_requeue_count: value.immediate_requeue_count,
            error_count: value.error_count,
            strategy_planned_total: value.strategy_planned_total,
            strategy_executed_total: value.strategy_executed_total,
            cancel_triggered_count: value.cancel_triggered_count,
            cancel_planned_total: value.cancel_planned_total,
            cancel_executed_total: value.cancel_executed_total,
            consumed_immediate_requeues: value.consumed_immediate_requeues,
        }
    }
}

#[pyclass(name = "ReconcileBatchItem")]
#[derive(Clone)]
pub(crate) struct PyReconcileBatchItem {
    #[pyo3(get)]
    offer_id: String,
    #[pyo3(get)]
    market_id: String,
    #[pyo3(get)]
    old_state: String,
    #[pyo3(get)]
    new_state: String,
    #[pyo3(get)]
    changed: bool,
    #[pyo3(get)]
    last_seen_status: Option<i64>,
    #[pyo3(get)]
    reason: String,
    #[pyo3(get)]
    taker_signal: String,
    #[pyo3(get)]
    taker_diagnostic: String,
    #[pyo3(get)]
    signal_source: String,
    #[pyo3(get)]
    coinset_tx_ids: Vec<String>,
    #[pyo3(get)]
    coinset_confirmed_tx_ids: Vec<String>,
    #[pyo3(get)]
    coinset_mempool_tx_ids: Vec<String>,
}

impl From<ReconcileBatchItem> for PyReconcileBatchItem {
    fn from(value: ReconcileBatchItem) -> Self {
        Self {
            offer_id: value.offer_id,
            market_id: value.market_id,
            old_state: value.old_state,
            new_state: value.new_state,
            changed: value.changed,
            last_seen_status: value.last_seen_status,
            reason: value.reason,
            taker_signal: value.taker_signal,
            taker_diagnostic: value.taker_diagnostic,
            signal_source: value.signal_source,
            coinset_tx_ids: value.coinset_tx_ids,
            coinset_confirmed_tx_ids: value.coinset_confirmed_tx_ids,
            coinset_mempool_tx_ids: value.coinset_mempool_tx_ids,
        }
    }
}

#[pyclass(name = "ReconcileBatchResult")]
pub(crate) struct PyReconcileBatchResult {
    #[pyo3(get)]
    items: Vec<Py<PyReconcileBatchItem>>,
    #[pyo3(get)]
    reconciled_count: u64,
    #[pyo3(get)]
    changed_count: u64,
}

impl PyReconcileBatchResult {
    fn from_engine(py: Python<'_>, batch: ReconcileBatchResult) -> PyResult<Self> {
        let items = batch
            .items
            .into_iter()
            .map(|item| Py::new(py, PyReconcileBatchItem::from(item)))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(Self {
            items,
            reconciled_count: batch.reconciled_count,
            changed_count: batch.changed_count,
        })
    }
}

#[pyclass(name = "BuildAndPostOfferRequest")]
#[derive(Clone)]
pub(crate) struct PyBuildAndPostOfferRequest {
    #[pyo3(get, set)]
    program_path: PathBuf,
    #[pyo3(get, set)]
    markets_path: PathBuf,
    #[pyo3(get, set)]
    testnet_markets_path: Option<PathBuf>,
    #[pyo3(get, set)]
    network: String,
    #[pyo3(get, set)]
    market_id: Option<String>,
    #[pyo3(get, set)]
    pair: Option<String>,
    #[pyo3(get, set)]
    size_base_units: u64,
    #[pyo3(get, set)]
    repeat: u32,
    #[pyo3(get, set)]
    publish_venue: Option<String>,
    #[pyo3(get, set)]
    dexie_base_url: Option<String>,
    #[pyo3(get, set)]
    splash_base_url: Option<String>,
    #[pyo3(get, set)]
    drop_only: bool,
    #[pyo3(get, set)]
    claim_rewards: bool,
    #[pyo3(get, set)]
    dry_run: bool,
    #[pyo3(get, set)]
    compact_json: bool,
    #[pyo3(get, set)]
    persist_results: bool,
    #[pyo3(get, set)]
    action_side: Option<String>,
}

#[pymethods]
impl PyBuildAndPostOfferRequest {
    #[allow(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (
        program_path,
        markets_path,
        network,
        size_base_units,
        *,
        testnet_markets_path=None,
        market_id=None,
        pair=None,
        repeat=1,
        publish_venue=None,
        dexie_base_url=None,
        splash_base_url=None,
        drop_only=true,
        claim_rewards=false,
        dry_run=false,
        compact_json=false,
        persist_results=true,
        action_side=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        program_path: PathBuf,
        markets_path: PathBuf,
        network: String,
        size_base_units: u64,
        testnet_markets_path: Option<PathBuf>,
        market_id: Option<String>,
        pair: Option<String>,
        repeat: u32,
        publish_venue: Option<String>,
        dexie_base_url: Option<String>,
        splash_base_url: Option<String>,
        drop_only: bool,
        claim_rewards: bool,
        dry_run: bool,
        compact_json: bool,
        persist_results: bool,
        action_side: Option<String>,
    ) -> Self {
        Self {
            program_path,
            markets_path,
            testnet_markets_path,
            network,
            market_id,
            pair,
            size_base_units,
            repeat,
            publish_venue,
            dexie_base_url,
            splash_base_url,
            drop_only,
            claim_rewards,
            dry_run,
            compact_json,
            persist_results,
            action_side,
        }
    }
}

impl PyBuildAndPostOfferRequest {
    fn into_engine(self) -> PyResult<BuildAndPostOfferRequest> {
        if self.market_id.is_none() == self.pair.is_none() {
            return Err(to_py_err(engine_core::Error::Other(
                "provide exactly one of market_id or pair".to_string(),
            )));
        }
        Ok(BuildAndPostOfferRequest {
            program_path: self.program_path,
            markets_path: self.markets_path,
            testnet_markets_path: self.testnet_markets_path,
            network: self.network,
            market_id: self.market_id,
            pair: self.pair,
            size_base_units: self.size_base_units,
            repeat: self.repeat,
            publish_venue: self.publish_venue,
            dexie_base_url: self.dexie_base_url,
            splash_base_url: self.splash_base_url,
            drop_only: self.drop_only,
            claim_rewards: self.claim_rewards,
            dry_run: self.dry_run,
            compact_json: self.compact_json,
            persist_results: self.persist_results,
            action_side: self.action_side,
        })
    }
}

#[pyclass(name = "BuildAndPostOfferResponse")]
pub(crate) struct PyBuildAndPostOfferResponse {
    #[pyo3(get)]
    exit_code: i32,
    #[pyo3(get)]
    output: String,
    #[pyo3(get)]
    payload: Py<PyAny>,
}

impl PyBuildAndPostOfferResponse {
    fn from_engine(py: Python<'_>, response: BuildAndPostOfferResponse) -> PyResult<Self> {
        Ok(Self {
            exit_code: response.exit_code,
            output: response.output,
            payload: dict_from_json_value(py, response.payload)?,
        })
    }
}

pub(crate) fn reconcile_offers_batch_typed(
    py: Python<'_>,
    db_path: PathBuf,
    dexie_base_url: String,
    target_venue: String,
    market_id: Option<String>,
    limit: usize,
) -> PyResult<Py<PyReconcileBatchResult>> {
    let market_filter = market_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let market_ref = market_filter.as_deref();
    let batch = py.detach(move || {
        runtime()
            .block_on(reconcile_offers_batch(
                &db_path,
                &dexie_base_url,
                &target_venue,
                market_ref,
                limit,
            ))
            .map_err(to_py_err)
    })?;
    Py::new(py, PyReconcileBatchResult::from_engine(py, batch)?)
}

pub(crate) fn build_and_post_offer_typed(
    py: Python<'_>,
    request: PyBuildAndPostOfferRequest,
) -> PyResult<Py<PyBuildAndPostOfferResponse>> {
    let engine_request = request.into_engine()?;
    let response = py.detach(move || {
        runtime()
            .block_on(build_and_post_offer(engine_request))
            .map_err(to_py_err)
    })?;
    Py::new(
        py,
        PyBuildAndPostOfferResponse::from_engine(py, response)?,
    )
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyDaemonCycleSummary>()?;
    m.add_class::<PyReconcileBatchItem>()?;
    m.add_class::<PyReconcileBatchResult>()?;
    m.add_class::<PyBuildAndPostOfferRequest>()?;
    m.add_class::<PyBuildAndPostOfferResponse>()?;
    Ok(())
}
