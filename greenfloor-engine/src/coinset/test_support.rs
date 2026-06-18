//! Shared coinset test fixtures (not production paths).

use chia_protocol::{Bytes32, Coin};
use chia_sdk_driver::{Cat, CatInfo};

/// Test puzzle hash byte derived from amount; truncates above 255 by design for fixtures.
#[allow(clippy::cast_possible_truncation)]
pub fn puzzle_byte_from_amount(amount: u64) -> u8 {
    amount as u8
}

pub fn cat_with_amount(amount: u64) -> Cat {
    Cat::new(
        Coin::new(
            Bytes32::new([puzzle_byte_from_amount(amount); 32]),
            Bytes32::default(),
            amount,
        ),
        None,
        CatInfo::new(Bytes32::new([0x01; 32]), None, Bytes32::default()),
    )
}
