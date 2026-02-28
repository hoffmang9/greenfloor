from __future__ import annotations

from datetime import UTC, datetime, timedelta
from typing import Any, cast

from greenfloor.config.models import MarketConfig, MarketInventoryConfig
from greenfloor.core.strategy import PlannedAction
from greenfloor.daemon import main as daemon_main
from greenfloor.daemon.main import (
    _active_offer_counts_by_size,
    _execute_strategy_actions,
    _inject_reseed_action_if_no_active_offers,
    _match_watched_coin_ids,
    _set_watched_coin_ids_for_market,
    _strategy_config_from_market,
    _update_market_coin_watchlist_from_dexie,
)


class _FakeDexie:
    def __init__(self, post_result: dict):
        self.post_result = post_result
        self.posted: list[str] = []
        self.calls = 0
        self.visible_offer_ids: set[str] = set()

    def post_offer(self, offer: str) -> dict:
        self.posted.append(offer)
        self.calls += 1
        return dict(self.post_result)

    def get_offer(self, offer_id: str) -> dict[str, Any]:
        clean_offer_id = str(offer_id).strip()
        if clean_offer_id in self.visible_offer_ids:
            return {"success": True, "offer": {"id": clean_offer_id, "status": 0}}
        raise RuntimeError("dexie_http_error:404")


class _FakeStore:
    def __init__(self) -> None:
        self.offer_states: list[dict] = []
        self.audit_events: list[dict] = []

    def upsert_offer_state(
        self, *, offer_id: str, market_id: str, state: str, last_seen_status: int | None
    ) -> None:
        self.offer_states.append(
            {
                "offer_id": offer_id,
                "market_id": market_id,
                "state": state,
                "last_seen_status": last_seen_status,
            }
        )

    def list_offer_states(self, *, market_id: str | None = None, limit: int = 200) -> list[dict]:
        _ = market_id, limit
        return list(self.offer_states)

    def list_recent_audit_events(
        self,
        *,
        event_types: list[str] | None = None,
        market_id: str | None = None,
        limit: int = 50,
    ) -> list[dict]:
        rows = list(self.audit_events)
        if event_types:
            allowed = set(event_types)
            rows = [row for row in rows if str(row.get("event_type", "")) in allowed]
        if market_id:
            rows = [row for row in rows if str(row.get("market_id", "")) == market_id]
        return rows[: int(limit)]

    def add_audit_event(self, event_type: str, payload: dict, market_id: str | None = None) -> None:
        self.audit_events.insert(
            0,
            {
                "event_type": str(event_type),
                "market_id": market_id,
                "payload": dict(payload),
            },
        )


def _market() -> MarketConfig:
    return MarketConfig(
        market_id="m1",
        enabled=True,
        base_asset="asset",
        base_symbol="BYC",
        quote_asset="xch",
        quote_asset_type="unstable",
        receive_address="xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        mode="sell_only",
        signer_key_id="key-main-1",
        inventory=MarketInventoryConfig(low_watermark_base_units=100),
        pricing={
            "fixed_quote_per_base": 0.5,
            "base_unit_mojo_multiplier": 1000,
            "quote_unit_mojo_multiplier": 1000,
        },
    )


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

    result = _execute_strategy_actions(
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


def test_execute_strategy_actions_skips_when_builder_skips(monkeypatch) -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._POST_COOLDOWN_UNTIL.clear()

    monkeypatch.setattr(
        daemon_main,
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

    result = _execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=32.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )

    assert result["planned_count"] == 1
    assert result["executed_count"] == 0
    assert result["items"][0]["status"] == "skipped"
    assert result["items"][0]["reason"] == "builder_not_ready"
    assert dexie.posted == []
    assert store.offer_states == []


def test_execute_strategy_actions_posts_and_persists_offer_ids(monkeypatch) -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._POST_COOLDOWN_UNTIL.clear()

    monkeypatch.setattr(
        daemon_main,
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

    result = _execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=32.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )

    assert result["planned_count"] == 2
    assert result["executed_count"] == 2
    assert len(dexie.posted) == 2
    assert len(store.offer_states) == 2
    assert all(s["offer_id"] == "offer-123" for s in store.offer_states)


def test_execute_strategy_actions_retries_then_succeeds(monkeypatch) -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "3")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", "10")

    monkeypatch.setattr(
        daemon_main,
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
    result = _execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )
    assert result["executed_count"] == 1
    assert dexie.calls == 2
    assert result["items"][0]["attempts"] == 2


def test_execute_strategy_actions_applies_post_cooldown_after_retry_exhaust(monkeypatch) -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "2")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", "60")
    monkeypatch.setattr(
        daemon_main,
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
    result = _execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )
    assert result["executed_count"] == 0
    assert dexie.calls == 2
    assert result["items"][0]["reason"].startswith("dexie_post_retry_exhausted:")
    assert result["items"][1]["reason"].startswith("post_cooldown_active:")


def test_build_offer_for_action_direct_builder_call(monkeypatch) -> None:
    monkeypatch.delenv("GREENFLOOR_OFFER_BUILDER_CMD", raising=False)
    captured = {}

    def _fake_build_offer(payload):
        captured["payload"] = payload
        return f"offer1direct-{payload['size_base_units']}"

    monkeypatch.setattr(
        "greenfloor.cli.offer_builder_sdk.build_offer",
        _fake_build_offer,
    )
    action = PlannedAction(
        size=10,
        repeat=1,
        pair="xch",
        expiry_unit="minutes",
        expiry_value=65,
        cancel_after_create=True,
        reason="below_target",
    )

    built = daemon_main._build_offer_for_action(
        market=_market(),
        action=action,
        xch_price_usd=31.5,
        network="mainnet",
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
    strategy_config = _strategy_config_from_market(market)

    actions = _inject_reseed_action_if_no_active_offers(
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
    strategy_config = _strategy_config_from_market(market)

    actions = _inject_reseed_action_if_no_active_offers(
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
    strategy_config = _strategy_config_from_market(market)

    actions = _inject_reseed_action_if_no_active_offers(
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
    strategy_config = _strategy_config_from_market(market)

    actions = _inject_reseed_action_if_no_active_offers(
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


def test_active_offer_counts_by_size_uses_offer_state_and_size_mapping() -> None:
    store = _FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {"offer_id": "one-1", "market_id": "m1", "state": "open"},
        {"offer_id": "ten-1", "market_id": "m1", "state": "refresh_due"},
        {
            "offer_id": "hundred-1",
            "market_id": "m1",
            "state": "mempool_observed",
            "updated_at": now.isoformat(),
        },
        {"offer_id": "unknown-1", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "payload": {
                "items": [
                    {"offer_id": "one-1", "size": 1, "status": "executed"},
                    {"offer_id": "ten-1", "size": 10, "status": "executed"},
                    {"offer_id": "hundred-1", "size": 100, "status": "executed"},
                ]
            },
        }
    ]

    counts, state_counts, unmapped = _active_offer_counts_by_size(
        store=cast(Any, store),
        market_id="m1",
        clock=now,
    )

    assert counts == {1: 1, 10: 1, 100: 1}
    assert state_counts["open"] == 2
    assert state_counts["refresh_due"] == 1
    assert state_counts["mempool_observed"] == 1
    assert unmapped == 1


def test_active_offer_counts_by_size_counts_cli_posted_offer() -> None:
    """CLI-posted offers must be counted by _active_offer_counts_by_size.

    Before the fix the CLI emitted strategy_offer_execution events without an
    items list, so _recent_offer_sizes_by_offer_id returned no size for the
    offer ID and it landed in active_unmapped_offer_ids instead of
    active_counts_by_size[100]. This caused the daemon to post a duplicate
    100-unit offer on every cycle.
    """
    store = _FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {"offer_id": "cli-hundred-1", "market_id": "m1", "state": "open"},
    ]
    # Event written by the fixed CLI path — has items with size/status/offer_id.
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "payload": {
                "market_id": "m1",
                "planned_count": 1,
                "executed_count": 1,
                "items": [
                    {
                        "size": 100,
                        "status": "executed",
                        "reason": "dexie_post_success",
                        "offer_id": "cli-hundred-1",
                        "attempts": 1,
                    }
                ],
                "venue": "dexie",
                "signature_request_id": "SignatureRequest_abc",
                "signature_state": "SUBMITTED",
            },
        }
    ]

    counts, state_counts, unmapped = _active_offer_counts_by_size(
        store=cast(Any, store),
        market_id="m1",
        clock=now,
    )

    assert counts == {1: 0, 10: 0, 100: 1}, "CLI-posted 100-unit offer must be counted"
    assert unmapped == 0, "CLI-posted offer must not appear in unmapped"


def test_update_market_coin_watchlist_from_dexie_tracks_coins_for_owned_offers() -> None:
    store = _FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [{"offer_id": "offer-1", "market_id": "m1", "state": "open"}]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "payload": {"offer_id": "offer-1"},
        }
    ]
    market = _market()
    offers = [
        {"id": "offer-1", "involved_coins": ["0x" + ("a" * 64)]},
        {"id": "offer-2", "involved_coins": ["0x" + ("b" * 64)]},
    ]

    _update_market_coin_watchlist_from_dexie(
        market=market,
        offers=cast(list[dict[str, Any]], offers),
        store=cast(Any, store),
        clock=now,
    )

    hits = _match_watched_coin_ids(observed_coin_ids=["a" * 64, "b" * 64])
    assert hits["m1"] == ["a" * 64]


def test_match_watched_coin_ids_returns_empty_without_overlap() -> None:
    _set_watched_coin_ids_for_market(market_id="m-empty", coin_ids={"c" * 64})
    assert _match_watched_coin_ids(observed_coin_ids=["d" * 64]) == {}


def test_resolve_quote_asset_for_offer_maps_symbol_from_cats(monkeypatch, tmp_path) -> None:
    cats = tmp_path / "cats.yaml"
    cats.write_text(
        "\n".join(
            [
                "cats:",
                "  - base_symbol: wUSDC.b",
                "    asset_id: fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d",
            ]
        ),
        encoding="utf-8",
    )
    monkeypatch.setattr(daemon_main, "_default_cats_config_path", lambda: cats)

    resolved = daemon_main._resolve_quote_asset_for_offer(
        quote_asset="wUSDC.b",
        network="mainnet",
    )
    assert resolved == "fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d"


def test_execute_strategy_actions_uses_cloud_wallet_path_when_configured(monkeypatch) -> None:
    daemon_main._POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setattr(
        daemon_main,
        "_cloud_wallet_offer_post_fallback",
        lambda **_kwargs: {"success": True, "offer_id": "offer-fallback-1"},
    )

    class _Program:
        cloud_wallet_base_url = "https://api.vault.chia.net"
        cloud_wallet_user_key_id = "UserAuthKey_abc"
        cloud_wallet_private_key_pem_path = "~/.greenfloor/keys/cloud-wallet-user-auth-key.pem"
        cloud_wallet_vault_id = "Wallet_abc"

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

    result = _execute_strategy_actions(
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
    assert result["items"][0]["reason"] == "cloud_wallet_post_success"


def test_execute_strategy_actions_cloud_wallet_requires_dexie_visibility(monkeypatch) -> None:
    daemon_main._POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setattr(
        daemon_main,
        "_cloud_wallet_offer_post_fallback",
        lambda **_kwargs: {"success": True, "offer_id": "offer-fallback-missing"},
    )

    class _Program:
        cloud_wallet_base_url = "https://api.vault.chia.net"
        cloud_wallet_user_key_id = "UserAuthKey_abc"
        cloud_wallet_private_key_pem_path = "~/.greenfloor/keys/cloud-wallet-user-auth-key.pem"
        cloud_wallet_vault_id = "Wallet_abc"

    dexie = _FakeDexie(post_result={"success": True, "id": "offer-1"})
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

    result = _execute_strategy_actions(
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
    assert "cloud_wallet_post_not_visible_on_dexie" in result["items"][0]["reason"]


def test_execute_strategy_actions_posts_larger_sizes_first(monkeypatch) -> None:
    daemon_main._POST_COOLDOWN_UNTIL.clear()
    seen_sizes: list[int] = []

    def _fake_cloud_wallet_post(**kwargs: Any) -> dict[str, Any]:
        seen_sizes.append(int(kwargs["size_base_units"]))
        size = int(kwargs["size_base_units"])
        return {"success": True, "offer_id": f"offer-{size}"}

    monkeypatch.setattr(daemon_main, "_cloud_wallet_offer_post_fallback", _fake_cloud_wallet_post)

    class _Program:
        cloud_wallet_base_url = "https://api.vault.chia.net"
        cloud_wallet_user_key_id = "UserAuthKey_abc"
        cloud_wallet_private_key_pem_path = "~/.greenfloor/keys/cloud-wallet-user-auth-key.pem"
        cloud_wallet_vault_id = "Wallet_abc"

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

    result = _execute_strategy_actions(
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
    assert seen_sizes == [100, 10, 1]


def test_execute_strategy_actions_cloud_wallet_failure_skips_without_builder(monkeypatch) -> None:
    daemon_main._POST_COOLDOWN_UNTIL.clear()
    calls = {"builder": 0}

    def _unexpected_builder(**_kwargs):
        calls["builder"] += 1
        return {"status": "executed", "reason": "offer_builder_success", "offer": "offer1unused"}

    monkeypatch.setattr(daemon_main, "_build_offer_for_action", _unexpected_builder)
    monkeypatch.setattr(
        daemon_main,
        "_cloud_wallet_offer_post_fallback",
        lambda **_kwargs: {"success": False, "error": "vault_signing_unavailable"},
    )

    class _Program:
        cloud_wallet_base_url = "https://api.vault.chia.net"
        cloud_wallet_user_key_id = "UserAuthKey_abc"
        cloud_wallet_private_key_pem_path = "~/.greenfloor/keys/cloud-wallet-user-auth-key.pem"
        cloud_wallet_vault_id = "Wallet_abc"

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

    result = _execute_strategy_actions(
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
    assert result["items"][0]["reason"] == "cloud_wallet_post_failed:vault_signing_unavailable"
    assert calls["builder"] == 0
