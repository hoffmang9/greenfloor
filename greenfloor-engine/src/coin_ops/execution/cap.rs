/// Resolve combine input cap once at `CoinOpExecContext` construction.
pub fn resolve_combine_input_cap() -> i64 {
    std::env::var("GREENFLOOR_COIN_OPS_COMBINE_INPUT_COIN_CAP")
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .map(|value| value.max(2))
        .unwrap_or(5)
}
