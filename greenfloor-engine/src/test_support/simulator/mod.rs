#[cfg(test)]
mod coinset_backend;
#[cfg(test)]
pub(crate) use coinset_backend::SimulatorOfferCoinset;
#[cfg(test)]
pub(crate) mod harness;
#[cfg(test)]
pub mod offer_roundtrips;
