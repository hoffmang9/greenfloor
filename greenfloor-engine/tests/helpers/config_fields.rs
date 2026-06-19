use serde_json::Value;

pub fn symbol_to_asset_id_pairs(fields: &Value) -> Vec<(String, String)> {
    let Some(raw) = fields.get("symbol_to_asset_id").and_then(Value::as_object) else {
        return Vec::new();
    };
    raw.iter()
        .filter_map(|(symbol, asset_id)| {
            let normalized = greenfloor_engine::hex::normalize_hex_id(asset_id.as_str()?);
            if normalized.is_empty() {
                None
            } else {
                Some((symbol.trim().to_ascii_lowercase(), normalized))
            }
        })
        .collect()
}

pub fn enabled_market_rows(fields: &Value) -> Vec<&Value> {
    fields
        .get("enabled_markets")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter(|row| row.is_object()).collect())
        .unwrap_or_default()
}
