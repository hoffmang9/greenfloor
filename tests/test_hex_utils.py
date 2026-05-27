from __future__ import annotations

from greenfloor.hex_utils import canonical_is_xch, is_xch_like_asset_id


def test_canonical_is_xch_requires_explicit_symbol() -> None:
    assert canonical_is_xch("xch")
    assert canonical_is_xch("TXCH")
    assert canonical_is_xch("1")
    assert not canonical_is_xch("")
    assert not canonical_is_xch("  ")
    assert not canonical_is_xch("a" * 64)


def test_is_xch_like_asset_id_matches_signer_empty_semantics() -> None:
    assert is_xch_like_asset_id("")
    assert is_xch_like_asset_id("xch")
    assert not is_xch_like_asset_id("a" * 64)
