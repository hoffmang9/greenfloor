from __future__ import annotations

import hashlib

import greenfloor.signing as signing


def _coin_id_bytes(parent_coin_info: bytes, puzzle_hash: bytes, amount: int) -> bytes:
    return hashlib.sha256(
        parent_coin_info + puzzle_hash + int(amount).to_bytes(8, "big", signed=False)
    ).digest()


class _FakeCoin:
    def __init__(self, parent_coin_info: bytes, puzzle_hash: bytes, amount: int) -> None:
        self.parent_coin_info = parent_coin_info
        self.puzzle_hash = puzzle_hash
        self.amount = int(amount)

    def coin_id(self) -> bytes:
        return _coin_id_bytes(self.parent_coin_info, self.puzzle_hash, self.amount)


class _FakeAddress:
    def __init__(self, puzzle_hash: bytes) -> None:
        self.puzzle_hash = puzzle_hash


class _FakeCreateCoin:
    def __init__(self, puzzle_hash: bytes, amount: int) -> None:
        self.puzzle_hash = puzzle_hash
        self.amount = int(amount)


class _FakeCondition:
    def __init__(self, puzzle_hash: bytes, amount: int) -> None:
        self._puzzle_hash = puzzle_hash
        self._amount = int(amount)

    def parse_create_coin(self) -> _FakeCreateCoin:
        return _FakeCreateCoin(self._puzzle_hash, self._amount)


class _FakeOutputValue:
    def __init__(self, condition: _FakeCondition) -> None:
        self._condition = condition

    def to_list(self) -> list[_FakeCondition]:
        return [self._condition]


class _FakeOutput:
    def __init__(self, condition: _FakeCondition) -> None:
        self.value = _FakeOutputValue(condition)


class _FakeCat:
    def __init__(self, coin: _FakeCoin) -> None:
        self.coin = coin


class _FakePuzzle:
    def __init__(self, sdk, child_puzzle_hash: bytes, child_amount: int) -> None:
        self._sdk = sdk
        self._child_puzzle_hash = child_puzzle_hash
        self._child_amount = int(child_amount)

    def parse_child_cats(self, parent_coin: _FakeCoin, _parent_solution) -> list[_FakeCat]:
        child_coin = self._sdk.Coin(
            parent_coin.coin_id(), self._child_puzzle_hash, self._child_amount
        )
        return [_FakeCat(child_coin)]


class _FakeProgram:
    """Program intentionally does not define parse_child_cats.

    This models the production regression where parse_child_cats exists on Puzzle,
    not Program. The code must call program.puzzle().parse_child_cats(...).
    """

    def __init__(self, sdk, child_puzzle_hash: bytes, child_amount: int) -> None:
        self._sdk = sdk
        self._child_puzzle_hash = child_puzzle_hash
        self._child_amount = int(child_amount)

    def puzzle(self) -> _FakePuzzle:
        return _FakePuzzle(self._sdk, self._child_puzzle_hash, self._child_amount)

    def run(self, _solution, _max_cost: int, _mempool_mode: bool) -> _FakeOutput:
        return _FakeOutput(_FakeCondition(self._child_puzzle_hash, self._child_amount))


class _FakeClvm:
    def __init__(self, sdk, child_puzzle_hash: bytes, child_amount: int) -> None:
        self._sdk = sdk
        self._child_puzzle_hash = child_puzzle_hash
        self._child_amount = int(child_amount)

    def deserialize(self, _blob: bytes):
        return _FakeProgram(self._sdk, self._child_puzzle_hash, self._child_amount)


class _FakeCoinsetAdapter:
    def __init__(
        self,
        *,
        cat_record: dict,
        parent_record: dict,
        parent_solution_record: dict,
    ) -> None:
        self._cat_record = cat_record
        self._parent_record = parent_record
        self._parent_solution_record = parent_solution_record

    def get_coin_records_by_puzzle_hash(self, *, puzzle_hash_hex: str, include_spent_coins: bool):
        _ = puzzle_hash_hex, include_spent_coins
        return [self._cat_record]

    def get_coin_record_by_name(self, *, coin_name_hex: str):
        _ = coin_name_hex
        return self._parent_record

    def get_puzzle_and_solution(self, *, coin_id_hex: str, height: int):
        _ = coin_id_hex, height
        return self._parent_solution_record


class _FakeSdk:
    def __init__(
        self, *, inner_puzzle_hash: bytes, cat_puzzle_hash: bytes, child_amount: int
    ) -> None:
        self._inner_puzzle_hash = inner_puzzle_hash
        self._cat_puzzle_hash = cat_puzzle_hash
        self._child_amount = int(child_amount)

        class _AddressNamespace:
            @staticmethod
            def decode(_address: str) -> _FakeAddress:
                return _FakeAddress(inner_puzzle_hash)

        self.Address = _AddressNamespace

    def Coin(self, parent_coin_info: bytes, puzzle_hash: bytes, amount: int) -> _FakeCoin:
        return _FakeCoin(parent_coin_info, puzzle_hash, amount)

    def Clvm(self) -> _FakeClvm:
        return _FakeClvm(self, self._cat_puzzle_hash, self._child_amount)

    def cat_puzzle_hash(self, _asset_id: bytes, _inner_puzzle_hash: bytes) -> bytes:
        return self._cat_puzzle_hash

    @staticmethod
    def to_hex(value: bytes) -> str:
        return bytes(value).hex()


def test_list_unspent_cat_coins_uses_puzzle_parse_child_cats(monkeypatch) -> None:
    asset_id = "11" * 32
    child_amount = 17634
    inner_puzzle_hash = bytes.fromhex("22" * 32)
    cat_puzzle_hash = bytes.fromhex("33" * 32)
    parent_parent_coin_id = bytes.fromhex("44" * 32)
    parent_puzzle_hash = bytes.fromhex("55" * 32)

    parent_coin_id = _coin_id_bytes(parent_parent_coin_id, parent_puzzle_hash, child_amount)
    child_coin_id = _coin_id_bytes(parent_coin_id, cat_puzzle_hash, child_amount)

    cat_record = {
        "coin": {
            "parent_coin_info": parent_coin_id.hex(),
            "puzzle_hash": cat_puzzle_hash.hex(),
            "amount": child_amount,
        }
    }
    parent_record = {
        "coin": {
            "parent_coin_info": parent_parent_coin_id.hex(),
            "puzzle_hash": parent_puzzle_hash.hex(),
            "amount": child_amount,
        },
        "spent_block_index": 123,
    }
    parent_solution_record = {
        "puzzle_reveal": "ff",
        "solution": "80",
    }

    fake_sdk = _FakeSdk(
        inner_puzzle_hash=inner_puzzle_hash,
        cat_puzzle_hash=cat_puzzle_hash,
        child_amount=child_amount,
    )
    fake_coinset = _FakeCoinsetAdapter(
        cat_record=cat_record,
        parent_record=parent_record,
        parent_solution_record=parent_solution_record,
    )

    monkeypatch.setattr(signing, "_coinset_adapter", lambda *, network: fake_coinset)

    cats = signing._list_unspent_cat_coins(
        sdk=fake_sdk,
        receive_address="txch1dummy",
        network="testnet11",
        asset_id=asset_id,
    )

    assert len(cats) == 1
    assert fake_sdk.to_hex(cats[0].coin.coin_id()) == child_coin_id.hex()
