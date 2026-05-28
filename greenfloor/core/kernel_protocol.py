"""Typed PyO3 surfaces for deterministic policy kernel bindings."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, Protocol

from greenfloor.core.coin_ops.kernel_protocol import CoinOpsKernelProtocol

from greenfloor.core.cycle_orchestration import (
    MarketBatchSelection,
    OfferStateRow,
    StaleSweepCandidate,
    StaleSweepHit,
    StaleSweepProgress,
)
from greenfloor.core.managed_action_outcome import ManagedActionOutcome
from greenfloor.core.managed_retry import ManagedRetryDecision
from greenfloor.core.parallel_batch_plan import ParallelBatchPlan
from greenfloor.core.parallel_reservation_context import ParallelReservationContext
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.strategy_action_item import StrategyActionItem

if TYPE_CHECKING:
    from greenfloor.core.cancel_policy import CancelPolicyDecision, OpenOfferRow
    from greenfloor.core.notifications import (
        LowInventoryEvaluation,
        LowInventoryInput,
    )


class CycleKernelProtocol(Protocol):
    def evaluate_market(self, state: Any, config: Any) -> list[PlannedAction]: ...

    def evaluate_two_sided_market_actions(
        self,
        buy_state: Any,
        sell_state: Any,
        buy_config: Any,
        sell_config: Any,
    ) -> list[PlannedAction]: ...

    def reseed_skip_reason_labels(self) -> tuple[str, ...]: ...

    def plan_reseed_actions_from_gap(
        self,
        strategy_actions: list[PlannedAction],
        active_counts_by_size: dict[int, int],
        target_counts_by_size: dict[int, int],
        strategy_config: Any,
        xch_price_usd: float | None,
    ) -> Any: ...

    def sequential_action_route(
        self,
        runtime_dry_run: bool,
        program_present: bool,
        managed_backend_available: bool,
    ) -> str: ...

    def expand_planned_actions(self, actions: list[PlannedAction]) -> list[PlannedAction]: ...

    def filter_planned_actions_with_positive_repeat(
        self,
        actions: list[PlannedAction],
    ) -> list[PlannedAction]: ...

    def plan_parallel_managed_dispatch(
        self,
        actions: list[PlannedAction],
        ctx: ParallelReservationContext,
        spendable_profiles: dict[str, dict[str, int | bool]],
    ) -> ParallelBatchPlan: ...

    def apply_offer_signal(self, state: str, signal: str) -> dict[str, Any]: ...

    def expiry_seconds_for_action(self, expiry_unit: str, expiry_value: int) -> int | None: ...

    def single_input_preferred_skip_reason(
        self,
        requested_amounts: dict[str, int],
        spendable_profiles: dict[str, dict[str, int | bool]],
    ) -> str | None: ...

    def is_transient_managed_upstream_error_text(self, error_text: str) -> bool: ...

    def classify_managed_transient_error(
        self, exception_class: str, error_text: str
    ) -> str | None: ...

    def is_managed_upstream_transient_error(
        self, exception_class: str, error_text: str
    ) -> bool: ...

    def is_managed_worker_transient_error(self, exception_class: str, error_text: str) -> bool: ...

    def is_parallel_dispatch_transient_error(
        self, exception_class: str, error_text: str
    ) -> bool: ...

    def is_transient_dexie_visibility_404_error(self, error: str) -> bool: ...

    def can_parallelize_managed_offers(
        self,
        signer_path_configured: bool,
        parallelism_enabled: bool,
        runtime_dry_run: bool,
        has_coordinator: bool,
    ) -> bool: ...

    def parallel_max_workers(self, submission_count: int, configured_max: int) -> int: ...

    def reservation_release_status(self, is_executed: bool) -> str: ...

    def should_apply_parallel_transient_cooldown(
        self,
        transient_failures: int,
        total_parallel: int,
        cooldown_seconds: int,
    ) -> bool: ...

    def managed_retry_decision(
        self,
        attempt_index: int,
        attempts_max: int,
        backoff_ms: int,
        is_upstream_transient: bool,
    ) -> ManagedRetryDecision: ...

    def classify_managed_post_result(
        self,
        success: bool,
        error_text: str,
        offer_id: str,
        publish_venue: str,
    ) -> ManagedActionOutcome: ...

    def classify_dexie_visibility_outcome(
        self,
        visible: bool,
        visibility_error: str,
    ) -> ManagedActionOutcome: ...

    def count_parallel_transient_failures(self, items: list[StrategyActionItem]) -> int: ...

    def select_market_batch(
        self,
        enabled_market_ids: list[str],
        slot_count: int,
        cursor: int,
        immediate_requeue_ids: list[str],
    ) -> MarketBatchSelection: ...

    def enqueue_immediate_requeue(
        self,
        immediate_requeue_ids: list[str],
        market_id: str,
    ) -> list[str]: ...

    def should_use_market_slot_dispatch(
        self, enabled_market_count: int, slot_count: int
    ) -> bool: ...

    def dedupe_sorted_market_ids(self, market_ids: list[str]) -> list[str]: ...

    def should_log_disabled_market(
        self, now_monotonic: float, next_log_deadline: float
    ) -> bool: ...

    def next_disabled_market_log_deadline(
        self, now_monotonic: float, interval_seconds: int
    ) -> float: ...

    def should_try_cat_inventory_fallback(
        self, coinset_scan_empty: bool, base_asset: str
    ) -> bool: ...

    def collect_stale_sweep_candidates(
        self,
        rows: list[OfferStateRow],
        enabled_market_ids: list[str],
        per_market_limit: int,
    ) -> list[StaleSweepCandidate]: ...

    def classify_dexie_stale_offer_status(self, status: int) -> str | None: ...

    def is_dexie_offer_missing_error_text(self, error_text: str) -> bool: ...

    def record_stale_sweep_check(
        self,
        progress: StaleSweepProgress,
        hit: StaleSweepHit | None,
    ) -> StaleSweepProgress: ...

    def needs_inventory_fallback(
        self, bucket_counts_available: bool, coinset_scan_empty: bool
    ) -> bool: ...

    def resolve_inventory_scan_source(
        self,
        coinset_scan_found_coins: bool,
        coinset_scan_empty: bool,
        cat_scan_found_coins: bool,
        wallet_scan_found_coins: bool,
    ) -> str: ...

    def resolve_tracked_sizes(
        self, ladder_sizes: list[int], strategy_default_sizes: list[int]
    ) -> list[int]: ...

    def is_two_sided_market_mode(self, market_mode: str) -> bool: ...

    def aggregate_two_sided_offer_counts(
        self,
        buy_counts: dict[int, int],
        sell_counts: dict[int, int],
        tracked_sizes: list[int],
    ) -> dict[int, int]: ...

    def one_sided_offer_counts_by_side(
        self,
        sell_counts: dict[int, int],
        tracked_sizes: list[int],
    ) -> dict[str, dict[int, int]]: ...


class CancelPolicyKernelProtocol(Protocol):
    def abs_move_bps(self, current: float | None, previous: float | None) -> float | None: ...

    def cancel_move_threshold_bps(
        self, market_threshold: int | None, env_threshold: int | None
    ) -> int: ...

    def evaluate_cancel_policy_decision(
        self,
        quote_asset_type: str,
        cancel_policy_stable_vs_unstable: bool,
        current_xch_price_usd: float | None,
        previous_xch_price_usd: float | None,
        market_threshold: int | None,
        env_threshold: int | None,
    ) -> CancelPolicyDecision: ...

    def collect_open_offer_ids_for_cancel(self, offers: list[OpenOfferRow]) -> list[str]: ...


class NotificationKernelProtocol(Protocol):
    def evaluate_low_inventory_alert(self, input: LowInventoryInput) -> LowInventoryEvaluation: ...


class OfferPolicyKernelProtocol(Protocol):
    def resolve_offer_expiry_for_pricing(self, pricing: dict[str, Any]) -> tuple[str, int]: ...

    def resolve_quote_price_for_pricing(self, pricing: dict[str, Any]) -> float: ...

    def mojo_multiplier_for_leg(self, pricing: dict[str, Any], field: str, asset_id: str) -> int: ...

    def verify_offer_for_dexie(self, offer: str) -> str | None: ...


class RetryPolicyKernelProtocol(Protocol):
    def parse_rate_limit_retry_seconds(self, error_text: str) -> float | None: ...

    def moderate_retry_sleep_seconds(
        self, current_sleep: float, rate_limit_wait: float | None
    ) -> float: ...

    def moderate_retry_next_sleep(self, current_sleep: float) -> float: ...

    def dexie_invalid_offer_should_retry(
        self, error: str, attempt: int, max_attempts: int
    ) -> bool: ...

    def dexie_invalid_offer_retry_sleep(self, attempt: int, initial_sleep: float) -> float: ...

    def coinset_fee_lookup_retry_sleep(self, attempt: int) -> float: ...

    def poll_exponential_sleep_now(
        self,
        elapsed_seconds: int,
        timeout_seconds: int,
        sleep_seconds: float,
        initial_sleep: float,
        max_sleep: float,
    ) -> float | None: ...

    def poll_exponential_advance_sleep(
        self,
        sleep_seconds: float,
        initial_sleep: float,
        max_sleep: float,
        multiplier: float,
    ) -> float: ...


class DeterministicPolicyKernelProtocol(
    CycleKernelProtocol,
    CancelPolicyKernelProtocol,
    NotificationKernelProtocol,
    Protocol,
):
    """Cycle, cancel, and notification deterministic policy bindings."""


class PolicyKernelProtocol(
    DeterministicPolicyKernelProtocol,
    CoinOpsKernelProtocol,
    OfferPolicyKernelProtocol,
    RetryPolicyKernelProtocol,
    Protocol,
):
    """Full in-process deterministic policy kernel (cycle, coin-ops, offer, retry)."""
