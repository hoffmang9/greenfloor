from __future__ import annotations

from datetime import UTC, datetime, timedelta

import pytest

from greenfloor.daemon.testing import (
    active_offer_counts_by_size,
    active_offer_counts_by_size_and_side,
    build_dexie_size_by_offer_id,
    match_watched_coin_ids,
    update_market_coin_watchlist_from_dexie,
)
from tests.helpers.daemon_test_fixtures import market_config
from tests.helpers.watchlist_store import (
    open_watchlist_test_store,
    seed_offer_state,
    seed_strategy_execution_event,
)

pytestmark = pytest.mark.usefixtures("engine_extension")


@pytest.fixture
def engine_extension() -> None:
    from greenfloor.core.engine_bridge import import_engine

    import_engine()


def test_active_offer_counts_by_size_uses_offer_state_and_size_mapping(tmp_path) -> None:
    store = open_watchlist_test_store(tmp_path)
    now = datetime.now(UTC)
    seed_offer_state(store, offer_id="one-1", market_id="m1", state="open", updated_at=now)
    seed_offer_state(store, offer_id="ten-1", market_id="m1", state="refresh_due", updated_at=now)
    seed_offer_state(
        store,
        offer_id="hundred-1",
        market_id="m1",
        state="mempool_observed",
        updated_at=now,
    )
    seed_offer_state(store, offer_id="unknown-1", market_id="m1", state="open", updated_at=now)
    seed_strategy_execution_event(
        store,
        market_id="m1",
        created_at=now,
        items=[
            {"offer_id": "one-1", "size": 1, "status": "executed"},
            {"offer_id": "ten-1", "size": 10, "status": "executed"},
            {"offer_id": "hundred-1", "size": 100, "status": "executed"},
        ],
    )

    counts, state_counts, unmapped = active_offer_counts_by_size(
        store=store,
        market_id="m1",
        clock=now,
    )

    assert counts == {1: 1, 10: 1, 100: 1}
    assert state_counts["open"] == 2
    assert state_counts["refresh_due"] == 1
    assert state_counts["mempool_observed"] == 1
    assert unmapped == 1


def test_active_offer_counts_by_size_counts_cli_posted_offer(tmp_path) -> None:
    store = open_watchlist_test_store(tmp_path)
    now = datetime.now(UTC)
    seed_offer_state(store, offer_id="cli-hundred-1", market_id="m1", state="open", updated_at=now)
    seed_strategy_execution_event(
        store,
        market_id="m1",
        created_at=now,
        items=[
            {
                "size": 100,
                "status": "executed",
                "reason": "dexie_post_success",
                "offer_id": "cli-hundred-1",
                "attempts": 1,
            }
        ],
    )

    counts, _, unmapped = active_offer_counts_by_size(store=store, market_id="m1", clock=now)

    assert counts == {1: 0, 10: 0, 100: 1}
    assert unmapped == 0


def test_active_offer_counts_by_size_and_side_unknown_metadata_stays_unmapped(tmp_path) -> None:
    store = open_watchlist_test_store(tmp_path)
    now = datetime.now(UTC)
    seed_offer_state(
        store, offer_id="offer-unknown-side", market_id="m1", state="open", updated_at=now
    )

    counts_by_side, state_counts, unmapped = active_offer_counts_by_size_and_side(
        store=store,
        market_id="m1",
        clock=now,
    )

    assert counts_by_side["buy"] == {1: 0, 10: 0, 100: 0}
    assert counts_by_side["sell"] == {1: 0, 10: 0, 100: 0}
    assert state_counts["open"] == 1
    assert unmapped == 1


def test_active_offer_counts_by_size_and_side_malformed_side_stays_unmapped(tmp_path) -> None:
    store = open_watchlist_test_store(tmp_path)
    now = datetime.now(UTC)
    seed_offer_state(store, offer_id="offer-bad-side", market_id="m1", state="open", updated_at=now)
    seed_offer_state(
        store, offer_id="offer-missing-side", market_id="m1", state="open", updated_at=now
    )
    seed_strategy_execution_event(
        store,
        market_id="m1",
        created_at=now,
        items=[
            {
                "offer_id": "offer-bad-side",
                "size": 10,
                "status": "executed",
                "side": "not-a-side",
            },
            {"offer_id": "offer-missing-side", "size": 10, "status": "executed"},
        ],
    )

    counts_by_side, _, unmapped = active_offer_counts_by_size_and_side(
        store=store,
        market_id="m1",
        clock=now,
    )

    assert counts_by_side["buy"] == {1: 0, 10: 0, 100: 0}
    assert counts_by_side["sell"] == {1: 0, 10: 0, 100: 0}
    assert unmapped == 2


def test_update_market_coin_watchlist_from_dexie_tracks_coins_for_owned_offers(tmp_path) -> None:
    store = open_watchlist_test_store(tmp_path)
    now = datetime.now(UTC)
    seed_offer_state(store, offer_id="offer-1", market_id="m1", state="open", updated_at=now)
    seed_strategy_execution_event(
        store,
        market_id="m1",
        created_at=now,
        items=[{"offer_id": "offer-1", "status": "executed"}],
    )
    market = market_config()
    offers = [
        {"id": "offer-1", "involved_coins": ["0x" + ("a" * 64)]},
        {"id": "offer-2", "involved_coins": ["0x" + ("b" * 64)]},
    ]

    update_market_coin_watchlist_from_dexie(
        market=market,
        offers=offers,
        store=store,
        clock=now,
    )

    hits = match_watched_coin_ids(observed_coin_ids=["a" * 64, "b" * 64])
    assert hits["m1"] == ["a" * 64]


def test_build_dexie_size_by_offer_id_extracts_sizes() -> None:
    base_asset = "asset-abc"
    offers = [
        {"id": "offer-1", "offered": [{"id": "asset-abc", "amount": 1}]},
        {"id": "offer-10", "offered": [{"id": "asset-abc", "amount": 10}]},
        {"id": "offer-100", "offered": [{"id": "asset-abc", "amount": 100}]},
        {"id": "offer-other", "offered": [{"id": "other-asset", "amount": 5}]},
    ]
    result = build_dexie_size_by_offer_id(offers, base_asset)
    assert result == {"offer-1": 1, "offer-10": 10, "offer-100": 100}
    assert "offer-other" not in result


def test_active_offer_counts_by_size_uses_dexie_hint_for_beyond_cap_offer(tmp_path) -> None:
    store = open_watchlist_test_store(tmp_path)
    now = datetime.now(UTC)
    seed_offer_state(
        store, offer_id="beyond-cap-hundred", market_id="m1", state="open", updated_at=now
    )

    counts_without, _, unmapped_without = active_offer_counts_by_size(
        store=store, market_id="m1", clock=now, dexie_size_by_offer_id={}
    )
    assert counts_without == {1: 0, 10: 0, 100: 0}
    assert unmapped_without == 1

    counts_with, _, unmapped_with = active_offer_counts_by_size(
        store=store,
        market_id="m1",
        clock=now,
        dexie_size_by_offer_id={"beyond-cap-hundred": 100},
    )
    assert counts_with == {1: 0, 10: 0, 100: 1}
    assert unmapped_with == 0


def test_active_offer_counts_by_size_foreign_offer_stays_unmapped(tmp_path) -> None:
    store = open_watchlist_test_store(tmp_path)
    now = datetime.now(UTC)
    seed_offer_state(store, offer_id="ours-100", market_id="m1", state="open", updated_at=now)
    seed_offer_state(store, offer_id="foreign-100", market_id="m1", state="open", updated_at=now)
    seed_strategy_execution_event(
        store,
        market_id="m1",
        created_at=now,
        items=[{"offer_id": "ours-100", "size": 100, "status": "executed"}],
    )

    counts, _, unmapped = active_offer_counts_by_size(store=store, market_id="m1", clock=now)

    assert counts == {1: 0, 10: 0, 100: 1}
    assert unmapped == 1


def test_active_offer_counts_by_size_tracks_non_legacy_size(tmp_path) -> None:
    store = open_watchlist_test_store(tmp_path)
    now = datetime.now(UTC)
    seed_offer_state(store, offer_id="ours-50", market_id="m1", state="open", updated_at=now)
    seed_strategy_execution_event(
        store,
        market_id="m1",
        created_at=now,
        items=[{"offer_id": "ours-50", "size": 50, "status": "executed"}],
    )
    counts, _, unmapped = active_offer_counts_by_size(
        store=store,
        market_id="m1",
        clock=now,
        tracked_sizes={1, 10, 50},
    )
    assert counts == {1: 0, 10: 0, 50: 1}
    assert unmapped == 0


def test_active_offer_counts_excludes_stale_pending_visibility_offer(tmp_path) -> None:
    store = open_watchlist_test_store(tmp_path)
    now = datetime.now(UTC)
    stale_created_at = now - timedelta(minutes=5)
    seed_offer_state(store, offer_id="pending-50", market_id="m1", state="open", updated_at=now)
    seed_strategy_execution_event(
        store,
        market_id="m1",
        created_at=stale_created_at,
        items=[
            {
                "offer_id": "pending-50",
                "size": 50,
                "status": "pending_visibility",
                "reason": "managed_offer_post_success",
            }
        ],
    )
    counts, _, unmapped = active_offer_counts_by_size(
        store=store,
        market_id="m1",
        clock=now,
        dexie_size_by_offer_id={},
        tracked_sizes={50},
    )
    assert counts == {50: 0}
    assert unmapped == 1


def test_active_offer_counts_keeps_pending_visibility_offer_when_seen_on_dexie(tmp_path) -> None:
    store = open_watchlist_test_store(tmp_path)
    now = datetime.now(UTC)
    stale_created_at = now - timedelta(minutes=5)
    seed_offer_state(store, offer_id="pending-50", market_id="m1", state="open", updated_at=now)
    seed_strategy_execution_event(
        store,
        market_id="m1",
        created_at=stale_created_at,
        items=[
            {
                "offer_id": "pending-50",
                "size": 50,
                "status": "pending_visibility",
                "reason": "managed_offer_post_success",
            }
        ],
    )
    counts, _, unmapped = active_offer_counts_by_size(
        store=store,
        market_id="m1",
        clock=now,
        dexie_size_by_offer_id={"pending-50": 50},
        tracked_sizes={50},
    )
    assert counts == {50: 1}
    assert unmapped == 0


def test_active_offer_counts_keeps_pending_when_no_dexie_snapshot(tmp_path) -> None:
    store = open_watchlist_test_store(tmp_path)
    now = datetime.now(UTC)
    very_old = now - timedelta(hours=1)
    seed_offer_state(store, offer_id="pending-old", market_id="m1", state="open", updated_at=now)
    seed_strategy_execution_event(
        store,
        market_id="m1",
        created_at=very_old,
        items=[
            {
                "offer_id": "pending-old",
                "size": 50,
                "status": "pending_visibility",
                "reason": "managed_offer_post_success",
            }
        ],
    )
    counts, _, unmapped = active_offer_counts_by_size(
        store=store,
        market_id="m1",
        clock=now,
        dexie_size_by_offer_id=None,
        tracked_sizes={50},
    )
    assert counts == {50: 1}
    assert unmapped == 0
