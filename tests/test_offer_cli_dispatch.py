from __future__ import annotations

import pytest

pytestmark = pytest.mark.skip(
    reason="offer CLI dispatch integration requires engine mocking unavailable via native subprocess"
)


def test_build_and_post_offer_cli_delegates_to_engine_in_process() -> None:
    pass
