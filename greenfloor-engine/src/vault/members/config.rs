use chia_sdk_driver::Restriction;
use clvm_utils::TreeHash;

#[derive(Debug, Clone, Default)]
pub struct MemberConfig {
    pub top_level: bool,
    pub nonce: u32,
    pub restrictions: Vec<Restriction>,
}

impl MemberConfig {
    #[must_use]
    pub fn with_top_level(&self, top_level: bool) -> Self {
        Self {
            top_level,
            ..self.clone()
        }
    }

    #[must_use]
    pub fn with_nonce(&self, nonce: u32) -> Self {
        Self {
            nonce,
            ..self.clone()
        }
    }

    #[must_use]
    pub fn with_restrictions(&self, restrictions: Vec<Restriction>) -> Self {
        Self {
            restrictions,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub struct WalletKey {
    pub public_key_hex: String,
    pub curve: String,
}

#[derive(Debug, Clone, Copy)]
pub struct P2ConditionsOrSingletonHashes {
    pub puzzle_hash: TreeHash,
    pub fixed_conditions_hash: TreeHash,
    pub p2_singleton_hash: TreeHash,
}
