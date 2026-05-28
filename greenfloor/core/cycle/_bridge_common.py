"""Shared helpers for cycle PyO3 bridge modules."""

from __future__ import annotations


def normalize_spendable_profiles(
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> dict[str, dict[str, int | bool]]:
    return {
        str(asset_id): {
            "total": int(profile.get("total", 0)),
            "max_single": int(profile.get("max_single", 0)),
            "max_single_known": bool(profile.get("max_single_known", False)),
        }
        for asset_id, profile in spendable_profiles.items()
    }
