from __future__ import annotations

import datetime as dt
from typing import cast

import pytest

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.runtime.cloud_wallet.coins import is_spendable_coin
from greenfloor.runtime.cloud_wallet.polling import (
    poll_offer_artifact_until_available,
    poll_signature_request_until_not_unsigned,
)

# _is_spendable_coin unit tests
# ---------------------------------------------------------------------------


def test_is_spendable_coin_allowlist_states_are_spendable() -> None:
    for state in ("CONFIRMED", "UNSPENT", "SPENDABLE", "AVAILABLE", "SETTLED"):
        assert is_spendable_coin({"state": state}) is True, state


def test_is_spendable_coin_known_non_spendable_states() -> None:
    for state in ("PENDING", "MEMPOOL", "SPENT", "SPENDING", "LOCKED", "RESERVED", "UNCONFIRMED"):
        assert is_spendable_coin({"state": state}) is False, state


def test_is_spendable_coin_unknown_state_is_not_spendable() -> None:
    assert is_spendable_coin({"state": "MYSTERY"}) is False
    assert is_spendable_coin({"state": "TRANSITIONING"}) is False


def test_is_spendable_coin_missing_state_is_not_spendable() -> None:
    assert is_spendable_coin({}) is False
    assert is_spendable_coin({"state": ""}) is False


def test_is_spendable_coin_locked_flag_is_not_spendable() -> None:
    assert is_spendable_coin({"state": "SETTLED", "isLocked": True}) is False


# ---------------------------------------------------------------------------
# _resolve_coin_global_ids unit tests
# ---------------------------------------------------------------------------


def test_poll_signature_request_returns_immediately_when_already_signed(monkeypatch) -> None:
    import time as time_module

    class _FakeWallet:
        @staticmethod
        def get_signature_request(*, signature_request_id):
            _ = signature_request_id
            return {"status": "SUBMITTED"}

    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: 0.0)

    status, events = poll_signature_request_until_not_unsigned(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        signature_request_id="sr-1",
        timeout_seconds=900,
        warning_interval_seconds=600,
    )
    assert status == "SUBMITTED"
    assert events == []


def test_poll_signature_request_emits_warning_event_after_interval(monkeypatch) -> None:
    import time as time_module

    call_count = [0]

    class _FakeWallet:
        @staticmethod
        def get_signature_request(*, signature_request_id):
            _ = signature_request_id
            call_count[0] += 1
            return {"status": "UNSIGNED" if call_count[0] < 2 else "SUBMITTED"}

    # monotonic: start=0, then elapsed=601 (triggers warning), then not called again
    elapsed_seq = iter([0.0, 601.0, 601.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))

    status, events = poll_signature_request_until_not_unsigned(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        signature_request_id="sr-1",
        timeout_seconds=900,
        warning_interval_seconds=600,
    )
    assert status == "SUBMITTED"
    warning_events = [e for e in events if e["event"] == "signature_wait_warning"]
    assert len(warning_events) == 1
    assert warning_events[0]["message"] == "still_waiting_on_user_signature"


def test_poll_signature_request_raises_on_timeout(monkeypatch) -> None:
    import time as time_module

    class _FakeWallet:
        @staticmethod
        def get_signature_request(*, signature_request_id):
            _ = signature_request_id
            return {"status": "UNSIGNED"}

    # start=0, then elapsed=901 immediately exceeds timeout=900
    elapsed_seq = iter([0.0, 901.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))

    with pytest.raises(RuntimeError, match="signature_request_timeout"):
        poll_signature_request_until_not_unsigned(
            wallet=_FakeWallet(),  # type: ignore[arg-type]
            signature_request_id="sr-1",
            timeout_seconds=900,
            warning_interval_seconds=600,
        )


def test_poll_signature_request_emits_escalation_on_repeated_warnings(monkeypatch) -> None:
    import time as time_module

    call_count = [0]

    class _FakeWallet:
        @staticmethod
        def get_signature_request(*, signature_request_id):
            _ = signature_request_id
            call_count[0] += 1
            if call_count[0] < 3:
                return {"status": "UNSIGNED"}
            return {"status": "SUBMITTED"}

    elapsed_seq = iter([0.0, 601.0, 1201.0, 1201.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))

    _status, events = poll_signature_request_until_not_unsigned(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        signature_request_id="sr-1",
        timeout_seconds=1800,
        warning_interval_seconds=600,
    )
    escalations = [e for e in events if e["event"] == "signature_wait_escalation"]
    assert len(escalations) == 1
    assert escalations[0]["warning_count"] == "2"


def test_poll_signature_request_retries_transient_errors(monkeypatch) -> None:
    import time as time_module

    call_count = [0]

    class _FakeWallet:
        @staticmethod
        def get_signature_request(*, signature_request_id):
            _ = signature_request_id
            call_count[0] += 1
            if call_count[0] == 1:
                raise RuntimeError("temporary")
            return {"status": "SUBMITTED"}

    elapsed_seq = iter([0.0, 0.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))

    _status, events = poll_signature_request_until_not_unsigned(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        signature_request_id="sr-1",
        timeout_seconds=900,
        warning_interval_seconds=600,
    )
    retries = [e for e in events if e["event"] == "poll_retry"]
    assert len(retries) == 1
    assert retries[0]["action"] == "wallet_get_signature_request"


# ---------------------------------------------------------------------------
# _wait_for_mempool_then_confirmation tests
# ---------------------------------------------------------------------------


def test_wait_for_mempool_emits_in_mempool_event_with_coinset_url(monkeypatch) -> None:
    import time as time_module

    from greenfloor.runtime.cloud_wallet.polling import (
        wait_for_mempool_then_confirmation as _wait_for_mempool_then_confirmation,
    )

    lc_call = [0]

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            lc_call[0] += 1
            if lc_call[0] == 1:
                return [{"id": "new-id", "name": "abc123hex", "state": "PENDING"}]
            return [{"id": "new-id", "name": "abc123hex", "state": "CONFIRMED"}]

    elapsed_seq = iter([0.0, 0.0, 0.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling._coinset_reconcile_coin_state",
        lambda **kwargs: {"reconcile": "ok", "confirmed_block_index": "10"},
    )
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling._watch_reorg_risk_with_coinset",
        lambda **kwargs: [{"event": "reorg_watch_complete"}],
    )

    events = _wait_for_mempool_then_confirmation(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        network="mainnet",
        initial_coin_ids=set(),
        mempool_warning_seconds=300,
        confirmation_warning_seconds=900,
    )
    in_mempool = [e for e in events if e["event"] == "in_mempool"]
    assert len(in_mempool) == 1
    assert in_mempool[0]["coinset_url"] == "https://coinset.org/coin/abc123hex"


def test_wait_for_mempool_returns_when_confirmed_coin_appears(monkeypatch) -> None:
    import time as time_module

    from greenfloor.runtime.cloud_wallet.polling import (
        wait_for_mempool_then_confirmation as _wait_for_mempool_then_confirmation,
    )

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            return [{"id": "new-id", "name": "confirmed-hex", "state": "CONFIRMED"}]

    elapsed_seq = iter([0.0, 0.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling._coinset_reconcile_coin_state",
        lambda **kwargs: {"reconcile": "ok", "confirmed_block_index": "10"},
    )
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling._watch_reorg_risk_with_coinset",
        lambda **kwargs: [{"event": "reorg_watch_complete"}],
    )

    events = _wait_for_mempool_then_confirmation(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        network="mainnet",
        initial_coin_ids=set(),
        mempool_warning_seconds=300,
        confirmation_warning_seconds=900,
    )
    # Returns successfully (no in_mempool event since we went straight to confirmed)
    assert all(e["event"] != "in_mempool" for e in events)


def test_wait_for_mempool_emits_warning_when_no_mempool_entry(monkeypatch) -> None:
    import time as time_module

    from greenfloor.runtime.cloud_wallet.polling import (
        wait_for_mempool_then_confirmation as _wait_for_mempool_then_confirmation,
    )

    lc_call = [0]

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            lc_call[0] += 1
            if lc_call[0] == 1:
                return []  # no new coins yet
            return [{"id": "new-id", "state": "CONFIRMED"}]

    # start=0, iteration-1 elapsed=300 (triggers mempool warning), iteration-2 returns
    elapsed_seq = iter([0.0, 300.0, 300.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling._coinset_reconcile_coin_state",
        lambda **kwargs: {"reconcile": "ok", "confirmed_block_index": "10"},
    )
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling._watch_reorg_risk_with_coinset",
        lambda **kwargs: [{"event": "reorg_watch_complete"}],
    )

    events = _wait_for_mempool_then_confirmation(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        network="mainnet",
        initial_coin_ids=set(),
        mempool_warning_seconds=300,
        confirmation_warning_seconds=900,
    )
    warning_events = [e for e in events if e["event"] == "mempool_wait_warning"]
    assert len(warning_events) == 1


def test_wait_for_mempool_ignores_coins_in_initial_set(monkeypatch) -> None:
    import time as time_module

    from greenfloor.runtime.cloud_wallet.polling import (
        wait_for_mempool_then_confirmation as _wait_for_mempool_then_confirmation,
    )

    lc_call = [0]

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            lc_call[0] += 1
            if lc_call[0] == 1:
                # only known initial coins — should not trigger pending or confirmed
                return [{"id": "old-id", "state": "CONFIRMED"}]
            return [
                {"id": "old-id", "state": "CONFIRMED"},
                {"id": "new-id", "state": "CONFIRMED"},
            ]

    elapsed_seq = iter([0.0, 0.0, 0.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling._coinset_reconcile_coin_state",
        lambda **kwargs: {"reconcile": "ok", "confirmed_block_index": "10"},
    )
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling._watch_reorg_risk_with_coinset",
        lambda **kwargs: [{"event": "reorg_watch_complete"}],
    )

    _wait_for_mempool_then_confirmation(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        network="mainnet",
        initial_coin_ids={"old-id"},
        mempool_warning_seconds=300,
        confirmation_warning_seconds=900,
    )
    # Returns because new-id appeared in confirmed, but old-id was ignored
    assert lc_call[0] == 2


def test_wait_for_mempool_filters_to_requested_asset(monkeypatch) -> None:
    import time as time_module

    from greenfloor.runtime.cloud_wallet.polling import (
        wait_for_mempool_then_confirmation as _wait_for_mempool_then_confirmation,
    )

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [
                {
                    "id": "other-coin",
                    "name": "other-name",
                    "state": "CONFIRMED",
                    "asset": {"id": "xch"},
                },
                {
                    "id": "target-coin",
                    "name": "target-name",
                    "state": "CONFIRMED",
                    "asset": {"id": "asset_target"},
                },
            ]

    elapsed_seq = iter([0.0, 0.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling._coinset_reconcile_coin_state",
        lambda **kwargs: {"reconcile": "ok", "confirmed_block_index": "10"},
    )
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling._watch_reorg_risk_with_coinset",
        lambda **kwargs: [{"event": "reorg_watch_complete"}],
    )

    events = _wait_for_mempool_then_confirmation(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        network="mainnet",
        initial_coin_ids=set(),
        asset_id="asset_target",
        mempool_warning_seconds=300,
        confirmation_warning_seconds=900,
    )
    confirmed = [e for e in events if e["event"] == "confirmed"]
    assert len(confirmed) == 1
    assert confirmed[0]["coin_name"] == "target-name"


def test_watch_reorg_risk_waits_until_additional_blocks(monkeypatch) -> None:
    import time as time_module

    from greenfloor.runtime.cloud_wallet.polling import _watch_reorg_risk_with_coinset

    peak_seq = iter([100, 102, 106])
    elapsed_seq = iter([0.0, 0.0, 10.0, 20.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling.mempool.coinset_peak_height",
        lambda **kwargs: next(peak_seq),
    )

    events = _watch_reorg_risk_with_coinset(
        network="mainnet",
        confirmed_block_index=100,
        additional_blocks=6,
        warning_interval_seconds=300,
    )
    assert events[0]["event"] == "reorg_watch_started"
    assert events[-1]["event"] == "reorg_watch_complete"


def test_watch_reorg_risk_times_out_when_chain_stalls(monkeypatch) -> None:
    import time as time_module

    from greenfloor.runtime.cloud_wallet.polling import _watch_reorg_risk_with_coinset

    elapsed_seq = iter([0.0, 0.0, 61.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.polling.mempool.coinset_peak_height", lambda **kwargs: 100
    )

    events = _watch_reorg_risk_with_coinset(
        network="mainnet",
        confirmed_block_index=100,
        additional_blocks=6,
        warning_interval_seconds=300,
        timeout_seconds=60,
    )
    assert events[0]["event"] == "reorg_watch_started"
    assert events[-1]["event"] == "reorg_watch_timeout"
    assert events[-1]["remaining_blocks"] == "6"


# ---------------------------------------------------------------------------
# _build_and_post_offer cloud wallet dispatch gate
# ---------------------------------------------------------------------------


def test_poll_offer_artifact_until_available_returns_new_offer(monkeypatch) -> None:
    wallets = [
        {
            "offers": [
                {
                    "offerId": "old-1",
                    "state": "OPEN",
                    "bech32": "offer1old",
                    "expiresAt": "2026-01-01T00:00:00Z",
                }
            ]
        },
        {
            "offers": [
                {
                    "offerId": "new-1",
                    "state": "OPEN",
                    "bech32": "offer1new",
                    "expiresAt": "2026-01-02T00:00:00Z",
                }
            ]
        },
    ]

    class _FakeWallet:
        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            if wallets:
                return wallets.pop(0)
            return {"offers": []}

    monotonic_tick = {"value": 0.0}

    def _mono() -> float:
        monotonic_tick["value"] += 1.0
        return float(monotonic_tick["value"])

    monkeypatch.setattr("time.sleep", lambda _seconds: None)
    monkeypatch.setattr("time.monotonic", _mono)

    offer = poll_offer_artifact_until_available(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        known_markers={"id:old-1", "bech32:offer1old"},
        timeout_seconds=10,
    )
    assert offer == "offer1new"


def test_poll_offer_artifact_until_available_filters_out_stale_created_at(monkeypatch) -> None:
    wallets = [
        {
            "offers": [
                {
                    "offerId": "stale-1",
                    "state": "OPEN",
                    "bech32": "offer1stale",
                    "expiresAt": "2026-01-03T00:00:00Z",
                    "createdAt": "2026-01-01T00:00:00Z",
                },
                {
                    "offerId": "new-1",
                    "state": "OPEN",
                    "bech32": "offer1new",
                    "expiresAt": "2026-01-04T00:00:00Z",
                    "createdAt": "2026-01-02T00:00:00Z",
                },
            ]
        }
    ]

    class _FakeWallet:
        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            if wallets:
                return wallets.pop(0)
            return {"offers": []}

    monotonic_tick = {"value": 0.0}

    def _mono() -> float:
        monotonic_tick["value"] += 0.5
        return float(monotonic_tick["value"])

    monkeypatch.setattr("time.sleep", lambda _seconds: None)
    monkeypatch.setattr("time.monotonic", _mono)

    offer = poll_offer_artifact_until_available(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        known_markers=set(),
        timeout_seconds=10,
        min_created_at=dt.datetime(2026, 1, 1, 12, 0, tzinfo=dt.UTC),
    )
    assert offer == "offer1new"


def test_poll_offer_artifact_until_available_times_out(monkeypatch) -> None:
    monotonic_values = iter([0.0, 5.0, 11.0])

    class _FakeWallet:
        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {"offers": []}

    monkeypatch.setattr("time.sleep", lambda _seconds: None)
    monkeypatch.setattr("time.monotonic", lambda: next(monotonic_values))

    try:
        poll_offer_artifact_until_available(
            wallet=cast(CloudWalletAdapter, _FakeWallet()),
            known_markers=set(),
            timeout_seconds=10,
        )
    except RuntimeError as exc:
        assert str(exc) == "cloud_wallet_offer_artifact_timeout"
    else:
        raise AssertionError("expected cloud_wallet_offer_artifact_timeout")


def test_poll_offer_artifact_until_available_requests_creator_open_pending(monkeypatch) -> None:
    calls: list[tuple[bool | None, list[str] | None, int]] = []

    class _FakeWallet:
        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=0):
            calls.append((is_creator, states, first))
            return {
                "offers": [
                    {
                        "offerId": "new-1",
                        "state": "OPEN",
                        "bech32": "offer1new",
                        "expiresAt": "2026-01-02T00:00:00Z",
                    }
                ]
            }

    monotonic_tick = {"value": 0.0}

    def _mono() -> float:
        monotonic_tick["value"] += 0.5
        return float(monotonic_tick["value"])

    monkeypatch.setattr("time.sleep", lambda _seconds: None)
    monkeypatch.setattr("time.monotonic", _mono)

    offer = poll_offer_artifact_until_available(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        known_markers=set(),
        timeout_seconds=10,
    )
    assert offer == "offer1new"
    assert calls
    assert calls[0][0] is True
    assert calls[0][1] == ["OPEN", "PENDING"]


def test_poll_offer_artifact_until_available_requires_open_state(monkeypatch) -> None:
    wallets = [
        {
            "offers": [
                {
                    "offerId": "pending-1",
                    "state": "PENDING",
                    "bech32": "offer1pending",
                    "expiresAt": "2026-01-03T00:00:00Z",
                    "createdAt": "2026-01-02T00:00:00Z",
                }
            ]
        },
        {
            "offers": [
                {
                    "offerId": "open-1",
                    "state": "OPEN",
                    "bech32": "offer1open",
                    "expiresAt": "2026-01-04T00:00:00Z",
                    "createdAt": "2026-01-03T00:00:00Z",
                }
            ]
        },
    ]
    monotonic_values = iter([0.0, 1.0, 1.0])

    class _FakeWallet:
        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            if wallets:
                return wallets.pop(0)
            return {"offers": []}

    monkeypatch.setattr("time.sleep", lambda _seconds: None)
    monkeypatch.setattr("time.monotonic", lambda: next(monotonic_values))

    offer = poll_offer_artifact_until_available(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        known_markers=set(),
        timeout_seconds=10,
        require_open_state=True,
    )
    assert offer == "offer1open"
