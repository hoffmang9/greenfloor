#[cfg(test)]
mod coinset_backend;
#[cfg(test)]
pub(crate) mod offer_cancel_fixtures;
#[cfg(test)]
mod offer_cancel_roundtrips;
#[cfg(test)]
mod offer_roundtrip_setup;
#[cfg(test)]
pub(crate) use coinset_backend::SimulatorOfferCoinset;
#[cfg(test)]
pub(crate) use offer_roundtrip_setup::{
    build_offer_from_setup, setup_roundtrip_opts, OfferRoundtripScenario,
};
#[cfg(test)]
pub(crate) mod harness;
#[cfg(test)]
pub mod offer_roundtrips;
