from __future__ import annotations

from datetime import UTC, datetime, timedelta
from typing import Any, cast

from greenfloor.daemon.testing import (
    active_offer_counts_by_size,
    active_offer_counts_by_size_and_side,
    build_dexie_size_by_offer_id,
    match_watched_coin_ids,
    update_market_coin_watchlist_from_dexie,
)
from tests.helpers.daemon_test_fixtures import FakeStore, market_config


def test_active_offer_counts_by_size_uses_offer_state_and_size_mapping() -> None:
    store = FakeStore()
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

    counts, state_counts, unmapped = active_offer_counts_by_size(
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
    """CLI-posted offers must be counted by active_offer_counts_by_size.

    Before the fix the CLI emitted strategy_offer_execution events without an
    items list, so _recent_offer_sizes_by_offer_id returned no size for the
    offer ID and it landed in active_unmapped_offer_ids instead of
    active_counts_by_size[100]. This caused the daemon to post a duplicate
    100-unit offer on every cycle.
    """
    store = FakeStore()
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

    counts, state_counts, unmapped = active_offer_counts_by_size(
        store=cast(Any, store),
        market_id="m1",
        clock=now,
    )

    assert counts == {1: 0, 10: 0, 100: 1}, "CLI-posted 100-unit offer must be counted"
    assert unmapped == 0, "CLI-posted offer must not appear in unmapped"


def test_active_offer_counts_by_size_and_side_unknown_metadata_stays_unmapped() -> None:
    store = FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {"offer_id": "offer-unknown-side", "market_id": "m1", "state": "open"},
    ]
    # No strategy_offer_execution audit event metadata for this active offer.
    store.audit_events = []

    counts_by_side, state_counts, unmapped = active_offer_counts_by_size_and_side(
        store=cast(Any, store),
        market_id="m1",
        clock=now,
    )

    assert counts_by_side["buy"] == {1: 0, 10: 0, 100: 0}
    assert counts_by_side["sell"] == {1: 0, 10: 0, 100: 0}
    assert state_counts["open"] == 1
    assert unmapped == 1


def test_active_offer_counts_by_size_and_side_malformed_side_stays_unmapped() -> None:
    store = FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {"offer_id": "offer-bad-side", "market_id": "m1", "state": "open"},
        {"offer_id": "offer-missing-side", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "payload": {
                "items": [
                    {
                        "offer_id": "offer-bad-side",
                        "size": 10,
                        "status": "executed",
                        "side": "not-a-side",
                    },
                    {
                        "offer_id": "offer-missing-side",
                        "size": 10,
                        "status": "executed",
                    },
                ]
            },
        }
    ]

    counts_by_side, state_counts, unmapped = active_offer_counts_by_size_and_side(
        store=cast(Any, store),
        market_id="m1",
        clock=now,
    )

    assert counts_by_side["buy"] == {1: 0, 10: 0, 100: 0}
    assert counts_by_side["sell"] == {1: 0, 10: 0, 100: 0}
    assert state_counts["open"] == 2
    assert unmapped == 2


def test_update_market_coin_watchlist_from_dexie_tracks_coins_for_owned_offers() -> None:
    store = FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [{"offer_id": "offer-1", "market_id": "m1", "state": "open"}]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "payload": {"offer_id": "offer-1"},
        }
    ]
    market = market_config()
    offers = [
        {"id": "offer-1", "involved_coins": ["0x" + ("a" * 64)]},
        {"id": "offer-2", "involved_coins": ["0x" + ("b" * 64)]},
    ]

    update_market_coin_watchlist_from_dexie(
        market=market,
        offers=cast(list[dict[str, Any]], offers),
        store=cast(Any, store),
        clock=now,
    )

    hits = match_watched_coin_ids(observed_coin_ids=["a" * 64, "b" * 64])
    assert hits["m1"] == ["a" * 64]


def test_build_dexie_size_by_offer_id_extracts_sizes() -> None:
    """build_dexie_size_by_offer_id maps offer IDs to base-unit sizes."""
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


def test_active_offer_counts_by_size_uses_dexie_hint_for_beyond_cap_offer() -> None:
    """Offers beyond the Dexie 20-offer cap must be resolved via dexie_size_by_offer_id.

    When we have more active offers than Dexie returns in its list endpoint, the
    beyond-cap offer won't be in the 20-offer response. The daemon fetches it
    individually from dexie.get_offer() and passes the result as dexie_size_by_offer_id.
    The ownership gate ensures only our own offers are in the DB, so this lookup is safe.
    """
    store = FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {"offer_id": "beyond-cap-hundred", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = []

    counts_without, _, unmapped_without = active_offer_counts_by_size(
        store=cast(Any, store), market_id="m1", clock=now
    )
    assert counts_without == {1: 0, 10: 0, 100: 0}
    assert unmapped_without == 1

    counts_with, _, unmapped_with = active_offer_counts_by_size(
        store=cast(Any, store),
        market_id="m1",
        clock=now,
        dexie_size_by_offer_id={"beyond-cap-hundred": 100},
    )
    assert counts_with == {1: 0, 10: 0, 100: 1}
    assert unmapped_with == 0


def test_active_offer_counts_by_size_foreign_offer_stays_unmapped() -> None:
    """Offers in the DB with no audit event entry must remain unmapped, never counted.

    This is the observable invariant enforced by the Dexie ownership gate: after the
    fix the Dexie state-update loop skips offers that are not in our_offer_ids, so
    foreign offers never reach the DB. If they somehow did, active_offer_counts_by_size
    must still not count them by size — they land in active_unmapped_offer_ids instead,
    keeping counts conservative and leaving a visible signal in the strategy_state_source
    log.
    """
    store = FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        # Our offer, correctly mapped via audit event.
        {"offer_id": "ours-100", "market_id": "m1", "state": "open"},
        # Foreign offer — in open state but no audit event (never posted by us).
        {"offer_id": "foreign-100", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "payload": {"items": [{"offer_id": "ours-100", "size": 100, "status": "executed"}]},
        }
    ]

    counts, _, unmapped = active_offer_counts_by_size(
        store=cast(Any, store),
        market_id="m1",
        clock=now,
    )

    assert counts == {1: 0, 10: 0, 100: 1}, "Only our mapped offer should be counted"
    assert unmapped == 1, "Foreign offer must stay unmapped, not inflate the count"


def test_active_offer_counts_by_size_tracks_non_legacy_size() -> None:
    store = FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {"offer_id": "ours-50", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "payload": {"items": [{"offer_id": "ours-50", "size": 50, "status": "executed"}]},
        }
    ]
    counts, _, unmapped = active_offer_counts_by_size(
        store=cast(Any, store),
        market_id="m1",
        clock=now,
        tracked_sizes={1, 10, 50},
    )
    assert counts == {1: 0, 10: 0, 50: 1}
    assert unmapped == 0


def test_active_offer_counts_excludes_stale_pending_visibility_offer() -> None:
    store = FakeStore()
    now = datetime.now(UTC)
    stale_created_at = (now - timedelta(minutes=5)).isoformat()
    store.offer_states = [
        {"offer_id": "pending-50", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "created_at": stale_created_at,
            "payload": {
                "items": [
                    {
                        "offer_id": "pending-50",
                        "size": 50,
                        "status": "pending_visibility",
                        "reason": "managed_offer_post_success",
                    }
                ]
            },
        }
    ]
    counts, _, unmapped = active_offer_counts_by_size(
        store=cast(Any, store),
        market_id="m1",
        clock=now,
        dexie_size_by_offer_id={},
        tracked_sizes={50},
    )
    assert counts == {50: 0}
    assert unmapped == 1


def test_active_offer_counts_keeps_pending_visibility_offer_when_seen_on_dexie() -> None:
    store = FakeStore()
    now = datetime.now(UTC)
    stale_created_at = (now - timedelta(minutes=5)).isoformat()
    store.offer_states = [
        {"offer_id": "pending-50", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "created_at": stale_created_at,
            "payload": {
                "items": [
                    {
                        "offer_id": "pending-50",
                        "size": 50,
                        "status": "pending_visibility",
                        "reason": "managed_offer_post_success",
                    }
                ]
            },
        }
    ]
    counts, _, unmapped = active_offer_counts_by_size(
        store=cast(Any, store),
        market_id="m1",
        clock=now,
        dexie_size_by_offer_id={"pending-50": 50},
        tracked_sizes={50},
    )
    assert counts == {50: 1}
    assert unmapped == 0


def test_active_offer_counts_keeps_pending_when_no_dexie_snapshot() -> None:
    """When dexie_size_by_offer_id is None (no Dexie snapshot this cycle),
    _is_stale_pending_visibility_offer returns False unconditionally, so the
    offer is not evicted regardless of age.
    """
    store = FakeStore()
    now = datetime.now(UTC)
    very_old = (now - timedelta(hours=1)).isoformat()
    store.offer_states = [
        {"offer_id": "pending-old", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "created_at": very_old,
            "payload": {
                "items": [
                    {
                        "offer_id": "pending-old",
                        "size": 50,
                        "status": "pending_visibility",
                        "reason": "managed_offer_post_success",
                    }
                ]
            },
        }
    ]
    counts, _, unmapped = active_offer_counts_by_size(
        store=cast(Any, store),
        market_id="m1",
        clock=now,
        dexie_size_by_offer_id=None,
        tracked_sizes={50},
    )
    assert counts == {50: 1}
    assert unmapped == 0
