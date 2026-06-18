from __future__ import annotations

from pathlib import Path

from tests.helpers.daemon_websocket_fixtures import write_markets
from tests.helpers.manager_cli import parse_json_output, run_manager
from tests.helpers.manager_program_fixtures import write_manager_program


def test_coins_list_requires_signer_backend(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "coins-list",
        ]
    )
    assert code == 2
    payload = parse_json_output(stdout)
    assert payload["error"] == "coin_list_requires_signer_backend"
