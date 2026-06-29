use chia_sdk_coinset::CoinRecord;

pub(crate) fn unspent_coin_records(records: Vec<CoinRecord>) -> impl Iterator<Item = CoinRecord> {
    records.into_iter().filter(|record| !record.spent)
}
