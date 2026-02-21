import shutil
from pathlib import Path

from greenfloor.cli.manager import _coin_op_budget_report
from greenfloor.storage.sqlite import SqliteStore


def test_manager_coin_op_budget_report(tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    shutil.copyfile("config/program.yaml", program)
    db = tmp_path / "state.sqlite"

    store = SqliteStore(db)
    try:
        store.add_coin_op_ledger_entry(
            market_id="m1",
            op_type="split",
            op_count=1,
            fee_mojos=10,
            status="executed",
            reason="stub_executed",
            operation_id="op-1",
        )
    finally:
        store.close()

    code = _coin_op_budget_report(program, str(db))
    assert code == 0
    out = capsys.readouterr().out
    assert '"spent_mojos": 10' in out
