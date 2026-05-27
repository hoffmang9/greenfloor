from __future__ import annotations

from tests.helpers.daemon_test_fixtures import *  # noqa: F403


def test_execute_strategy_actions_dry_run_plans_without_posting() -> None:
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-1"})
    dexie.visible_offer_ids = {"offer-100", "offer-10", "offer-1"}
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        )
    ]

    result = execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=True,
        xch_price_usd=32.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )

    assert result["planned_count"] == 2
    assert result["executed_count"] == 0
    assert len(result["items"]) == 2
    assert dexie.posted == []
    assert store.offer_states == []


def test_expand_strategy_actions_preserves_strategy_order() -> None:
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="below_target",
        ),
        PlannedAction(
            size=10,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="below_target",
        ),
    ]

    expanded = expand_strategy_actions(actions)

    assert [action.size for action in expanded] == [1, 1, 10, 10]


def test_execute_strategy_actions_skips_when_builder_skips(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()

    monkeypatch.setattr(
        strategy_dispatch,
        "_build_offer_for_action",
        lambda **_kwargs: {"status": "skipped", "reason": "builder_not_ready", "offer": None},
    )
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-1"})
    dexie.visible_offer_ids = {"offer-100", "offer-10", "offer-1"}
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        )
    ]

    result = _execute_local_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=32.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        program=minimal_program_config(),
    )

    assert result["planned_count"] == 1
    assert result["executed_count"] == 0
    assert result["items"][0]["status"] == "skipped"
    assert result["items"][0]["reason"] == "builder_not_ready"
    assert dexie.posted == []
    assert store.offer_states == []


def test_execute_strategy_actions_posts_and_persists_offer_ids(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()

    monkeypatch.setattr(
        strategy_dispatch,
        "_build_offer_for_action",
        lambda **_kwargs: {
            "status": "executed",
            "reason": "offer_builder_success",
            "offer": "offer1abc",
        },
    )
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-123"})
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=10,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        )
    ]

    result = _execute_local_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=32.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        program=minimal_program_config(),
    )

    assert result["planned_count"] == 2
    assert result["executed_count"] == 2
    assert len(dexie.posted) == 2
    assert len(store.offer_states) == 2
    assert all(s["offer_id"] == "offer-123" for s in store.offer_states)
    first_item = result["items"][0]
    assert isinstance(first_item.get("offer_create_ms"), int)
    assert isinstance(first_item.get("offer_publish_ms"), int)
    assert isinstance(first_item.get("offer_total_ms"), int)


def test_execute_strategy_actions_retries_then_succeeds(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "3")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", "10")

    monkeypatch.setattr(
        strategy_dispatch,
        "_build_offer_for_action",
        lambda **_kwargs: {"status": "executed", "reason": "ok", "offer": "offer1abc"},
    )

    class _FlakyDexie:
        def __init__(self) -> None:
            self.calls = 0

        def post_offer(self, _offer: str) -> dict:
            self.calls += 1
            if self.calls < 2:
                return {"success": False, "error": "temporary"}
            return {"success": True, "id": "offer-retry"}

    dexie = _FlakyDexie()
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        )
    ]
    result = _execute_local_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        program=minimal_program_config(),
    )
    assert result["executed_count"] == 1
    assert dexie.calls == 2
    assert result["items"][0]["attempts"] == 2


def test_execute_strategy_actions_applies_post_cooldown_after_retry_exhaust(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "2")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", "60")
    monkeypatch.setattr(
        strategy_dispatch,
        "_build_offer_for_action",
        lambda **_kwargs: {"status": "executed", "reason": "ok", "offer": "offer1abc"},
    )

    dexie = _FakeDexie(post_result={"success": False, "error": "down"})
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        )
    ]
    result = _execute_local_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        program=minimal_program_config(),
    )
    assert result["executed_count"] == 0
    assert dexie.calls == 2
    assert result["items"][0]["reason"].startswith("dexie_post_retry_exhausted:")
    assert result["items"][1]["reason"].startswith("post_cooldown_active:")


def test_build_offer_for_action_direct_builder_call(monkeypatch) -> None:
    captured = {}

    def _fake_build_offer(payload):
        captured["payload"] = payload
        return f"offer1direct-{payload['size_base_units']}"

    monkeypatch.setattr("greenfloor.offer_builder.build_offer", _fake_build_offer)
    action = PlannedAction(
        size=10,
        repeat=1,
        pair="xch",
        expiry_unit="minutes",
        expiry_value=65,
        cancel_after_create=True,
        reason="below_target",
    )

    built = build_offer_for_action(
        program=minimal_program_config(),
        market=_market(),
        action=action,
        xch_price_usd=31.5,
        keyring_yaml_path="/tmp/keyring.yaml",
    )

    assert built["status"] == "executed"
    assert built["reason"] == "offer_builder_success"
    assert built["offer"] == "offer1direct-10"
    assert captured["payload"]["quote_price_quote_per_base"] == 0.5
    assert captured["payload"]["base_unit_mojo_multiplier"] == 1000
    assert captured["payload"]["quote_unit_mojo_multiplier"] == 1000
    assert captured["payload"]["key_id"] == "key-main-1"
    assert captured["payload"]["network"] == "mainnet"
    assert captured["payload"]["keyring_yaml_path"] == "/tmp/keyring.yaml"


def test_inject_reseed_action_when_no_active_offers() -> None:
    store = _FakeStore()
    store.offer_states = [{"offer_id": "old-1", "market_id": "m1", "state": "expired"}]
    market = _market()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=datetime.now(UTC),
    )

    assert [action.size for action in actions] == [1, 10, 100]
    assert [action.repeat for action in actions] == [5, 2, 1]
    assert all(action.reason == "offer_size_gap_reseed" for action in actions)


def test_inject_reseed_action_skips_when_offer_targets_are_satisfied() -> None:
    store = _FakeStore()
    store.offer_states = [
        *[{"offer_id": f"one-{idx}", "market_id": "m1", "state": "open"} for idx in range(1, 6)],
        *[{"offer_id": f"ten-{idx}", "market_id": "m1", "state": "open"} for idx in range(1, 3)],
        {"offer_id": "hundred-1", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "payload": {
                "items": [
                    {"offer_id": f"one-{idx}", "size": 1, "status": "executed"}
                    for idx in range(1, 6)
                ]
                + [
                    {"offer_id": f"ten-{idx}", "size": 10, "status": "executed"}
                    for idx in range(1, 3)
                ]
                + [{"offer_id": "hundred-1", "size": 100, "status": "executed"}]
            },
        }
    ]
    market = _market()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=datetime.now(UTC),
    )

    assert actions == []


def test_inject_reseed_action_fills_missing_sizes_when_recent_mempool_is_present() -> None:
    store = _FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {
            "offer_id": "mempool-1",
            "market_id": "m1",
            "state": "mempool_observed",
            "updated_at": now.isoformat(),
        }
    ]
    market = _market()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=now,
    )

    assert [action.size for action in actions] == [1, 10, 100]
    assert [action.repeat for action in actions] == [5, 2, 1]
    assert all(action.reason == "offer_size_gap_reseed" for action in actions)


def test_inject_reseed_action_when_only_mempool_offer_is_stale() -> None:
    store = _FakeStore()
    now = datetime.now(UTC)
    stale = now - timedelta(minutes=31)
    store.offer_states = [
        {
            "offer_id": "mempool-old-1",
            "market_id": "m1",
            "state": "mempool_observed",
            "updated_at": stale.isoformat(),
        }
    ]
    market = _market()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=now,
    )

    assert [action.size for action in actions] == [1, 10, 100]
    assert [action.repeat for action in actions] == [5, 2, 1]
    assert all(action.reason == "offer_size_gap_reseed" for action in actions)


def test_inject_reseed_action_refills_missing_same_size_offers_immediately() -> None:
    store = _FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {"offer_id": "one-1", "market_id": "m1", "state": "open"},
        {"offer_id": "one-2", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "created_at": (now - timedelta(seconds=60)).isoformat(),
            "payload": {
                "items": [
                    {"offer_id": "recent-one", "size": 1, "status": "executed"},
                    {"offer_id": "one-1", "size": 1, "status": "executed"},
                    {"offer_id": "one-2", "size": 1, "status": "executed"},
                ]
            },
        }
    ]
    market = _market()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=now,
    )

    assert [action.size for action in actions] == [1, 10, 100]
    assert [action.repeat for action in actions] == [3, 2, 1]


def test_inject_reseed_action_is_not_limited_by_old_cadence_window() -> None:
    store = _FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {"offer_id": "one-1", "market_id": "m1", "state": "open"},
        {"offer_id": "one-2", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "created_at": (now - timedelta(minutes=4)).isoformat(),
            "payload": {
                "items": [
                    {"offer_id": "stale-one", "size": 1, "status": "executed"},
                    {"offer_id": "one-1", "size": 1, "status": "executed"},
                    {"offer_id": "one-2", "size": 1, "status": "executed"},
                ]
            },
        }
    ]
    market = _market()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=now,
    )

    assert [action.size for action in actions] == [1, 10, 100]
    assert [action.repeat for action in actions] == [3, 2, 1]


def test_drop_zero_repeat_strategy_actions_preserves_positive_repeats() -> None:
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="below_target",
            side="sell",
        )
    ]

    filtered = drop_zero_repeat_strategy_actions(actions)

    assert filtered == actions


def test_drop_zero_repeat_strategy_actions_filters_zero_repeat_actions() -> None:
    actions = [
        PlannedAction(
            size=1,
            repeat=0,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="below_target",
            side="sell",
        ),
        PlannedAction(
            size=1,
            repeat=1,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="below_target",
            side="sell",
        ),
    ]

    filtered = drop_zero_repeat_strategy_actions(actions)

    assert len(filtered) == 1
    assert filtered[0].repeat == 1


def test_execute_strategy_actions_uses_signer_managed_path_when_configured(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-fallback-1"},
    )

    _Program = _signer_program_config

    dexie = _FakeDexie(post_result={"success": True, "id": "offer-1"})
    dexie.visible_offer_ids = {"offer-fallback-1"}
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
        )
    ]

    result = execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
    )

    assert result["executed_count"] == 1
    assert result["items"][0]["reason"] == "managed_offer_post_success"


def test_execute_strategy_actions_signer_managed_requires_dexie_visibility(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setattr("time.sleep", lambda _seconds: None)
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-fallback-missing"},
    )

    _Program = _signer_program_config

    class _DexieNon404:
        def get_offer(self, offer_id: str) -> dict[str, Any]:
            _ = offer_id
            raise RuntimeError("dexie_http_error:500")

    store = _FakeStore()
    actions = [
        PlannedAction(
            size=100,
            repeat=1,
            pair="usdc",
            expiry_unit="hours",
            expiry_value=8,
            cancel_after_create=True,
            reason="offer_size_gap_reseed",
        )
    ]

    result = execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, _DexieNon404()),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
    )

    assert result["executed_count"] == 0
    assert result["items"][0]["status"] == "skipped"
    assert "managed_offer_post_not_visible_on_dexie" in result["items"][0]["reason"]


def test_execute_strategy_actions_signer_managed_accepts_transient_dexie_http_404(
    monkeypatch,
) -> None:
    """A transient 404 from Dexie is treated as pending-visibility, not a hard failure.

    The offer is counted as executed with _PENDING_VISIBILITY_REASON so the
    active-offer reader keeps it in scope until the grace period expires.
    """
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setattr("time.sleep", lambda _seconds: None)
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-fallback-pending"},
    )

    _Program = _signer_program_config

    class _Dexie404:
        def get_offer(self, offer_id: str) -> dict[str, Any]:
            _ = offer_id
            raise RuntimeError("HTTP Error 404: Not Found")

    store = _FakeStore()
    actions = [
        PlannedAction(
            size=50,
            repeat=1,
            pair="usdc",
            expiry_unit="hours",
            expiry_value=8,
            cancel_after_create=True,
            reason="offer_size_gap_reseed",
        )
    ]

    result = execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, _Dexie404()),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
    )

    assert result["executed_count"] == 1
    assert result["items"][0]["status"] == "executed"
    assert result["items"][0]["reason"] == PENDING_VISIBILITY_REASON
    assert result["items"][0]["offer_id"] == "offer-fallback-pending"


def test_execute_strategy_actions_preserves_planned_size_order(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()
    seen_sizes: list[int] = []

    def _fake_managed_offer_post(**kwargs: Any) -> dict[str, Any]:
        seen_sizes.append(int(kwargs["size_base_units"]))
        size = int(kwargs["size_base_units"])
        return {"success": True, "offer_id": f"offer-{size}"}

    monkeypatch.setattr(strategy_dispatch, "_managed_offer_post", _fake_managed_offer_post)

    _Program = _signer_program_config

    dexie = _FakeDexie(post_result={"success": True, "id": "offer-1"})
    dexie.visible_offer_ids = {"offer-100", "offer-10", "offer-1"}
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="hours",
            expiry_value=8,
            cancel_after_create=True,
            reason="offer_size_gap_reseed",
        ),
        PlannedAction(
            size=10,
            repeat=1,
            pair="usdc",
            expiry_unit="hours",
            expiry_value=8,
            cancel_after_create=True,
            reason="offer_size_gap_reseed",
        ),
        PlannedAction(
            size=100,
            repeat=1,
            pair="usdc",
            expiry_unit="hours",
            expiry_value=8,
            cancel_after_create=True,
            reason="offer_size_gap_reseed",
        ),
    ]

    result = execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
    )

    assert result["executed_count"] == 3
    assert seen_sizes == [1, 10, 100]


def test_execute_strategy_actions_signer_managed_failure_skips_without_builder(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()
    calls = {"builder": 0}

    def _unexpected_builder(**_kwargs):
        calls["builder"] += 1
        return {"status": "executed", "reason": "offer_builder_success", "offer": "offer1unused"}

    monkeypatch.setattr(strategy_dispatch, "_build_offer_for_action", _unexpected_builder)
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": False, "error": "vault_signing_unavailable"},
    )

    _Program = _signer_program_config

    dexie = _FakeDexie(post_result={"success": True, "id": "offer-1"})
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
        )
    ]

    result = execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
    )

    assert result["executed_count"] == 0
    assert result["items"][0]["status"] == "skipped"
    assert result["items"][0]["reason"] == "managed_offer_post_failed:vault_signing_unavailable"
    assert calls["builder"] == 0


def test_execute_strategy_actions_parallel_signer_managed_reservation_contention(
    monkeypatch,
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(self, *, include_pending: bool = True):
            _ = include_pending
            return [
                {
                    "amount": 1500,
                    "state": "SPENDABLE",
                    "asset": {"id": "asset"},
                },
                {
                    "amount": 10_000_000,
                    "state": "SPENDABLE",
                    "asset": {"id": "xch_asset"},
                },
            ]

    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"amount": 5000, "state": "CONFIRMED", "id": "coin-base", "name": "coin-base"},
            {"amount": 10_000_000, "state": "CONFIRMED", "id": "coin-xch", "name": "coin-xch"},
        ],
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_resolve_signer_offer_asset_ids_for_reservation",
        lambda **_kwargs: ("asset", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-parallel"},
    )

    def _Program() -> ProgramConfig:
        return _signer_program_config(runtime_offer_parallelism_enabled=True)

    class _DeterministicContentionCoordinator:
        def __init__(self) -> None:
            self._lock = threading.Lock()
            self.non_empty_acquire_calls = 0
            self.released: list[tuple[str, str]] = []

        def try_acquire(self, **kwargs):
            requested = dict(kwargs.get("requested_amounts", {}) or {})
            if not requested:
                # Daemon health-check path.
                return SimpleNamespace(ok=True, reservation_id="res-health", error=None)
            with self._lock:
                self.non_empty_acquire_calls += 1
                if self.non_empty_acquire_calls == 1:
                    return SimpleNamespace(ok=True, reservation_id="res-1", error=None)
                return SimpleNamespace(
                    ok=False,
                    reservation_id=None,
                    error="reservation_insufficient_asset",
                )

        def release(self, *, reservation_id: str, status: str) -> None:
            self.released.append((str(reservation_id), str(status)))

        def probe_storage(self) -> None:
            return None

    coordinator = _DeterministicContentionCoordinator()
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-parallel"})
    dexie.visible_offer_ids = {"offer-parallel"}
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
        )
    ]
    result = execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
        reservation_coordinator=cast(Any, coordinator),
    )
    assert result["planned_count"] == 2
    assert result["executed_count"] == 1
    assert any("reservation_insufficient_asset" in str(item["reason"]) for item in result["items"])
    assert coordinator.released == [("res-1", "released_success")]


def test_execute_strategy_actions_parallel_releases_reservation_on_failure(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(self, *, include_pending: bool = True):
            _ = include_pending
            return [
                {
                    "amount": 5000,
                    "state": "SPENDABLE",
                    "asset": {"id": "asset"},
                },
                {
                    "amount": 10_000_000,
                    "state": "SPENDABLE",
                    "asset": {"id": "xch_asset"},
                },
            ]

    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"amount": 5000, "state": "CONFIRMED", "id": "coin-base", "name": "coin-base"},
            {"amount": 10_000_000, "state": "CONFIRMED", "id": "coin-xch", "name": "coin-xch"},
        ],
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_resolve_signer_offer_asset_ids_for_reservation",
        lambda **_kwargs: ("asset", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": False, "error": "vault_unavailable"},
    )

    def _Program() -> ProgramConfig:
        return _signer_program_config(runtime_offer_parallelism_enabled=True)

    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-parallel"})
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
        )
    ]
    result = execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
        reservation_coordinator=coordinator,
    )
    assert result["executed_count"] == 0
    sqlite_store = SqliteStore(db_path)
    try:
        rows = sqlite_store.list_offer_reservation_leases()
        assert len(rows) == 1
        assert rows[0]["status"] == "released_failed"
    finally:
        sqlite_store.close()


def test_reservation_coordinator_expires_stale_leases(tmp_path) -> None:
    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=30)
    store = SqliteStore(db_path)
    try:
        store.add_offer_reservation_lease(
            reservation_id="res-stale",
            market_id="m1",
            wallet_id="Wallet_abc",
            asset_amounts={"asset": 1000},
            lease_seconds=30,
        )
        rows = store.list_offer_reservation_leases(reservation_id="res-stale")
        assert rows[0]["status"] == "active"
    finally:
        store.close()
    store = SqliteStore(db_path)
    try:
        store.expire_offer_reservation_leases(now=datetime.now(UTC) + timedelta(hours=1))
    finally:
        store.close()
    assert coordinator.expire_stale() == 0
    store = SqliteStore(db_path)
    try:
        rows = store.list_offer_reservation_leases(reservation_id="res-stale")
        assert rows[0]["status"] == "expired"
    finally:
        store.close()


def test_execute_strategy_actions_parallel_does_not_reserve_coin_ops_min_fee(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(self, *, include_pending: bool = True):
            _ = include_pending
            return [
                {"amount": 5000, "state": "SPENDABLE", "asset": {"id": "asset"}},
                {"amount": 10, "state": "SPENDABLE", "asset": {"id": "xch_asset"}},
            ]

    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"amount": 5000, "state": "CONFIRMED", "id": "coin-base", "name": "coin-base"},
            {"amount": 10_000_000, "state": "CONFIRMED", "id": "coin-xch", "name": "coin-xch"},
        ],
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_resolve_signer_offer_asset_ids_for_reservation",
        lambda **_kwargs: ("asset", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-parallel"},
    )

    def _Program():
        return replace(
            _signer_program_config(runtime_offer_parallelism_enabled=True),
            coin_ops_minimum_fee_mojos=10,
        )

    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-parallel"})
    dexie.visible_offer_ids = {"offer-parallel"}
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
        )
    ]
    result = execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
        reservation_coordinator=coordinator,
    )
    assert result["executed_count"] == 2
    assert all(
        "reservation_insufficient_xch_asset" not in str(item["reason"]) for item in result["items"]
    )


def test_execute_strategy_actions_parallel_falls_back_to_sequential_on_transient_reservation_error(
    monkeypatch,
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _ContentionCoordinator:
        def probe_storage(self) -> None:
            raise ReservationContentionError("reservation contention on asset lock")

    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"amount": 5000, "state": "CONFIRMED", "id": "coin-base", "name": "coin-base"},
            {"amount": 10_000_000, "state": "CONFIRMED", "id": "coin-xch", "name": "coin-xch"},
        ],
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_resolve_signer_offer_asset_ids_for_reservation",
        lambda **_kwargs: ("asset", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-fallback"},
    )

    def _Program() -> ProgramConfig:
        return _signer_program_config(runtime_offer_parallelism_enabled=True)

    dexie = _FakeDexie(post_result={"success": True, "id": "offer-fallback"})
    dexie.visible_offer_ids = {"offer-fallback"}
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
        )
    ]
    result = execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
        reservation_coordinator=cast(Any, _ContentionCoordinator()),
    )
    assert result["executed_count"] == 1
    assert any(event["event_type"] == "offer_parallel_fallback" for event in store.audit_events)


def test_execute_strategy_actions_parallel_raises_on_non_transient_reservation_error(
    monkeypatch,
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _BrokenCoordinator:
        def probe_storage(self) -> None:
            raise ReservationStorageError("reservation_storage_down")

    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"amount": 5000, "state": "CONFIRMED", "id": "coin-base", "name": "coin-base"},
            {"amount": 10_000_000, "state": "CONFIRMED", "id": "coin-xch", "name": "coin-xch"},
        ],
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_resolve_signer_offer_asset_ids_for_reservation",
        lambda **_kwargs: ("asset", "quote_asset", "xch_asset"),
    )

    def _Program() -> ProgramConfig:
        return _signer_program_config(runtime_offer_parallelism_enabled=True)

    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
        )
    ]
    with pytest.raises(ReservationStorageError, match="reservation_storage_down"):
        execute_strategy_actions(
            market=_market(),
            strategy_actions=actions,
            runtime_dry_run=False,
            xch_price_usd=30.0,
            dexie=cast(Any, _FakeDexie(post_result={"success": True, "id": "offer-fallback"})),
            store=cast(Any, _FakeStore()),
            publish_venue="dexie",
            program=_Program(),
            reservation_coordinator=cast(Any, _BrokenCoordinator()),
        )


def test_execute_strategy_actions_parallel_sets_post_cooldown_on_transient_worker_failures(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "1")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", "60")

    class _FakeManagedSigner:
        def list_coins(self, *, asset_id: str | None = None, include_pending: bool = True):
            _ = asset_id, include_pending
            return [{"amount": 20_000, "state": "SETTLED", "asset": {"id": "asset_global"}}]

    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"amount": 5000, "state": "CONFIRMED", "id": "coin-base", "name": "coin-base"},
            {"amount": 10_000_000, "state": "CONFIRMED", "id": "coin-xch", "name": "coin-xch"},
        ],
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_resolve_signer_offer_asset_ids_for_reservation",
        lambda **_kwargs: ("asset_global", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_execute_single_managed_action",
        lambda **_kwargs: (_ for _ in ()).throw(TimeoutError("The read operation timed out")),
    )

    def _Program() -> ProgramConfig:
        return _signer_program_config(runtime_offer_parallelism_enabled=True)

    market = _market()
    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = _FakeDexie(post_result={"success": True, "id": "unused"})
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
            side="sell",
        )
    ]
    result = execute_strategy_actions(
        market=market,
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
        reservation_coordinator=coordinator,
    )
    assert result["executed_count"] == 0
    assert all(
        str(item.get("reason", "")).startswith("parallel_offer_worker_error:")
        for item in result["items"]
    )
    remaining_ms = cooldown_remaining_ms(
        POST_COOLDOWN_UNTIL,
        f"dexie:{market.market_id}",
    )
    assert remaining_ms > 0


def test_execute_strategy_actions_signer_managed_nonparallel_converts_worker_exception_to_skip(
    monkeypatch,
) -> None:
    def _Program():
        return _signer_program_config(runtime_offer_parallelism_enabled=False)

    dexie = _FakeDexie(post_result={"success": True, "id": "unused"})
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
            side="sell",
        )
    ]
    monkeypatch.setattr(
        strategy_dispatch,
        "_execute_single_managed_action",
        lambda **_kwargs: (_ for _ in ()).throw(TimeoutError("The read operation timed out")),
    )

    result = execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
        reservation_coordinator=None,
    )
    assert result["executed_count"] == 0
    assert len(result["items"]) == 1
    assert str(result["items"][0]["reason"]).startswith("managed_action_error:")


def test_execute_strategy_actions_parallel_uses_resolved_asset_ids_for_reservation(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(self, *, include_pending: bool = True):
            _ = include_pending
            return [
                {"amount": 1500, "state": "SPENDABLE", "asset": {"id": "asset_global"}},
                {"amount": 10_000_000, "state": "SPENDABLE", "asset": {"id": "xch_asset"}},
            ]

    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"amount": 5000, "state": "CONFIRMED", "id": "coin-base", "name": "coin-base"},
            {"amount": 10_000_000, "state": "CONFIRMED", "id": "coin-xch", "name": "coin-xch"},
        ],
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_resolve_signer_offer_asset_ids_for_reservation",
        lambda **_kwargs: ("asset_global", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-resolved-asset"},
    )

    def _Program() -> ProgramConfig:
        return _signer_program_config(runtime_offer_parallelism_enabled=True)

    market = _market()
    market.base_asset = "asset-local-only"
    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-resolved-asset"})
    dexie.visible_offer_ids = {"offer-resolved-asset"}
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
        )
    ]
    result = execute_strategy_actions(
        market=market,
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
        reservation_coordinator=coordinator,
    )
    assert result["executed_count"] == 1


def test_execute_strategy_actions_parallel_uses_asset_scoped_coin_inventory(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(
            self, *, asset_id: str | None = None, include_pending: bool = True
        ) -> list[dict[str, Any]]:
            _ = include_pending
            # Simulate the wallet behavior that motivated asset-scoped filtering:
            # a broad unfiltered query reports pending-only inventory.
            if not asset_id:
                return [
                    {
                        "amount": 9_999_999_999_000,
                        "state": "PENDING",
                        "asset": {"id": "xch_asset"},
                    }
                ]
            if asset_id == "asset_global":
                return [{"amount": 1500, "state": "SETTLED", "asset": {"id": "asset_global"}}]
            if asset_id == "xch_asset":
                return [{"amount": 1000, "state": "SETTLED", "asset": {"id": "xch_asset"}}]
            return []

    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"amount": 5000, "state": "CONFIRMED", "id": "coin-base", "name": "coin-base"},
            {"amount": 10_000_000, "state": "CONFIRMED", "id": "coin-xch", "name": "coin-xch"},
        ],
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_resolve_signer_offer_asset_ids_for_reservation",
        lambda **_kwargs: ("asset_global", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-scoped"},
    )

    def _Program() -> ProgramConfig:
        return _signer_program_config(runtime_offer_parallelism_enabled=True)

    market = _market()
    market.base_asset = "asset-local-only"
    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-scoped"})
    dexie.visible_offer_ids = {"offer-scoped"}
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
        )
    ]
    result = execute_strategy_actions(
        market=market,
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
        reservation_coordinator=coordinator,
    )
    assert result["executed_count"] == 1
    assert not any(
        "reservation_insufficient_asset" in str(item["reason"]) for item in result["items"]
    )


def test_execute_strategy_actions_parallel_prefers_single_input_offer(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(
            self, *, asset_id: str | None = None, include_pending: bool = True
        ) -> list[dict[str, Any]]:
            _ = include_pending
            if asset_id == "asset_global":
                return [
                    {
                        "id": "c1",
                        "amount": 600,
                        "state": "SETTLED",
                        "asset": {"id": "asset_global"},
                    },
                    {
                        "id": "c2",
                        "amount": 600,
                        "state": "SETTLED",
                        "asset": {"id": "asset_global"},
                    },
                ]
            if asset_id == "xch_asset":
                return [
                    {"id": "x1", "amount": 1000, "state": "SETTLED", "asset": {"id": "xch_asset"}}
                ]
            return []

    monkeypatch.setattr(
        strategy_dispatch,
        "_resolve_signer_offer_asset_ids_for_reservation",
        lambda **_kwargs: ("asset_global", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-should-not-post"},
    )

    def _Program() -> ProgramConfig:
        return _signer_program_config(runtime_offer_parallelism_enabled=True)

    market = _market()
    market.base_asset = "asset-local-only"
    market.pricing = {"fixed_quote_per_base": 1.0, "base_unit_mojo_multiplier": 1000}
    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-should-not-post"})
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
            side="sell",
        )
    ]

    def _list_unspent_coins(**kwargs: Any) -> list[dict[str, Any]]:
        asset_id = str(kwargs.get("asset_id", "")).strip()
        if asset_id == "asset_global":
            return [
                {"amount": 600, "state": "CONFIRMED", "id": "c1", "name": "c1"},
                {"amount": 600, "state": "CONFIRMED", "id": "c2", "name": "c2"},
            ]
        if asset_id == "xch_asset":
            return [{"amount": 1000, "state": "CONFIRMED", "id": "x1", "name": "x1"}]
        return []

    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        _list_unspent_coins,
    )
    result = execute_strategy_actions(
        market=market,
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=_Program(),
        reservation_coordinator=coordinator,
    )
    assert result["executed_count"] == 0
    assert any(
        "single_input_preferred_requires_combine" in str(item["reason"]) for item in result["items"]
    )


def test_coinset_spendable_base_unit_coin_amounts_filters_and_converts(monkeypatch) -> None:
    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"amount": 10000, "state": "CONFIRMED"},
            {"amount": 999, "state": "CONFIRMED"},
            {"amount": 20000, "state": "PENDING"},
        ],
    )
    got = coinset_spendable_base_unit_coin_amounts(
        program=_signer_program_config(),
        market=_market(),
        resolved_asset_id="Asset_byc",
        base_unit_mojo_multiplier=1000,
    )
    assert got == [10]


def test_single_input_preferred_skip_reason_ignores_unknown_max_single() -> None:
    reason = single_input_preferred_skip_reason(
        requested_amounts={"Asset_byc": 10000},
        spendable_profiles={
            "Asset_byc": {
                "total": 77000,
                "max_single": 0,
                "coin_count": 0,
                "max_single_known": 0,
            }
        },
    )
    assert reason is None


def test_select_spendable_coins_for_target_amount_prefers_exact() -> None:
    coins = [
        {"id": "c5", "amount": 5000},
        {"id": "c3", "amount": 3000},
        {"id": "c2", "amount": 2000},
        {"id": "c3b", "amount": 3000},
    ]
    coin_ids, total, exact = select_spendable_coins_for_target_amount(
        coins=coins,
        target_amount=10_000,
    )
    assert exact is True
    assert total == 10_000
    assert set(coin_ids) == {"c5", "c3", "c2"}


def test_select_spendable_coins_for_target_amount_uses_change_when_needed() -> None:
    coins = [
        {"id": "c5", "amount": 5000},
        {"id": "c3a", "amount": 3000},
        {"id": "c3b", "amount": 3000},
    ]
    coin_ids, total, exact = select_spendable_coins_for_target_amount(
        coins=coins,
        target_amount=10_000,
    )
    assert exact is False
    assert total == 11_000
    assert set(coin_ids) == {"c5", "c3a", "c3b"}


def test_reservation_coordinator_cross_instance_contention_allows_single_winner(
    tmp_path,
) -> None:
    db_path = tmp_path / "reservations.sqlite"
    coordinator_a = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    coordinator_b = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    barrier = threading.Barrier(2)
    results: list[tuple[bool, str | None]] = []
    results_lock = threading.Lock()

    def _attempt(coordinator: AssetReservationCoordinator) -> None:
        barrier.wait()
        acquired = coordinator.try_acquire(
            market_id="m1",
            wallet_id="wallet-1",
            requested_amounts={"asset": 100},
            available_amounts={"asset": 100},
        )
        with results_lock:
            results.append((acquired.ok, acquired.error))

    thread_a = threading.Thread(target=_attempt, args=(coordinator_a,))
    thread_b = threading.Thread(target=_attempt, args=(coordinator_b,))
    thread_a.start()
    thread_b.start()
    thread_a.join()
    thread_b.join()

    assert len(results) == 2
    success_count = sum(1 for ok, _ in results if ok)
    failure_count = sum(1 for ok, _ in results if not ok)
    assert success_count == 1
    assert failure_count == 1
    assert any("reservation_insufficient_asset" in str(error) for ok, error in results if not ok)
