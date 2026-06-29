mod payload;
mod record;

pub use payload::{coin_records_from_payload, record_from_payload};
pub use record::{coin_from_record, coin_id_from_record, coin_spend_from_solution_payload};

#[cfg(test)]
mod tests;
