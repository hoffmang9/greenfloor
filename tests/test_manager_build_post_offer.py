from __future__ import annotations

from pathlib import Path

import pytest

from tests.helpers.engine_binary import (
    GreenfloorEngineBinaryError,
    resolve_greenfloor_engine_binary,
)
from tests.helpers.manager_cli import run_manager
from tests.helpers.manager_program_fixtures import write_manager_program


def test_resolve_greenfloor_engine_binary_from_env(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    binary = tmp_path / "greenfloor-engine"
    binary.write_text("#!/bin/sh\n", encoding="utf-8")
    binary.chmod(0o755)
    monkeypatch.setenv("GREENFLOOR_ENGINE_BIN", str(binary))
    assert resolve_greenfloor_engine_binary() == binary


def test_resolve_greenfloor_engine_binary_missing_env(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv("GREENFLOOR_ENGINE_BIN", raising=False)
    monkeypatch.setattr(
        "tests.helpers.engine_binary.shutil.which",
        lambda _name: None,
    )
    monkeypatch.setattr(
        "tests.helpers.engine_binary.repo_root",
        lambda: Path("/nonexistent"),
    )
    with pytest.raises(GreenfloorEngineBinaryError, match="binary not found"):
        resolve_greenfloor_engine_binary(build_if_missing=False)


@pytest.mark.skip(
    reason="build-and-post-offer integration requires engine mocking unavailable via native subprocess"
)
def test_build_and_post_offer_delegates_to_engine() -> None:
    pass


@pytest.mark.skip(
    reason="build-and-post-offer integration requires engine mocking unavailable via native subprocess"
)
def test_build_and_post_offer_dry_run_delegates() -> None:
    pass


@pytest.mark.skip(
    reason="build-and-post-offer integration requires engine mocking unavailable via native subprocess"
)
def test_build_and_post_offer_returns_nonzero_when_publish_fails() -> None:
    pass


def test_build_and_post_offer_rejects_invalid_repeat(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    markets.write_text("markets: []\n", encoding="utf-8")

    code, _stdout, stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "build-and-post-offer",
            "--market-id",
            "m1",
            "--size-base-units",
            "1",
            "--repeat",
            "0",
            "--network",
            "mainnet",
        ]
    )
    assert code != 0
    assert "repeat must be positive" in stderr
