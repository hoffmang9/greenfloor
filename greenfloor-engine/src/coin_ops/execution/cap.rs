pub fn combine_input_coin_cap() -> i64 {
    std::env::var("GREENFLOOR_COIN_OPS_COMBINE_INPUT_COIN_CAP")
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .map(|value| value.max(2))
        .unwrap_or(5)
}
