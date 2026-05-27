"""Tests for OfferPostRequest helpers."""

from __future__ import annotations

from greenfloor.runtime.offer_post_request import parse_managed_offer_post_result


def test_parse_managed_offer_post_result_success() -> None:
    payload = {
        "publish_failures": 0,
        "results": [
            {
                "result": {
                    "success": True,
                    "id": "offer-42",
                    "timing_ms": {
                        "create_total_ms": 100,
                        "publish_ms": 50,
                        "total_ms": 160,
                        "create_phase_ms": 80,
                        "artifact_wait_ms": 20,
                    },
                }
            }
        ],
    }
    result = parse_managed_offer_post_result(0, payload)
    assert result == {
        "success": True,
        "offer_id": "offer-42",
        "error": "",
        "offer_create_ms": 100,
        "offer_publish_ms": 50,
        "offer_total_ms": 160,
        "offer_create_phase_ms": 80,
        "offer_artifact_wait_ms": 20,
    }


def test_parse_managed_offer_post_result_nonzero_exit_code() -> None:
    payload = {
        "publish_failures": 1,
        "results": [
            {
                "result": {
                    "success": False,
                    "error": "bootstrap_pending:split_submitted",
                    "timing_ms": {"create_total_ms": 12, "publish_ms": None},
                }
            }
        ],
    }
    result = parse_managed_offer_post_result(2, payload)
    assert result["success"] is False
    assert result["error"] == "bootstrap_pending:split_submitted"
    assert result["offer_create_ms"] == 12
    assert result["offer_publish_ms"] is None


def test_parse_managed_offer_post_result_missing_results() -> None:
    result = parse_managed_offer_post_result(0, {"publish_failures": 0, "results": []})
    assert result == {"success": False, "error": "managed_offer_post_missing_results"}


def test_parse_managed_offer_post_result_publish_failure_with_zero_exit() -> None:
    payload = {
        "publish_failures": 1,
        "results": [{"result": {"success": True, "id": "offer-1", "timing_ms": {}}}],
    }
    result = parse_managed_offer_post_result(0, payload)
    assert result["success"] is False
    assert result["offer_id"] == "offer-1"
    assert result["error"] == ""
