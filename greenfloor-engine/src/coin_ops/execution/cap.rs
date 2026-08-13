/// Resolve combine input cap from program config (minimum 2, default 5).
#[must_use]
pub fn resolve_combine_input_cap(configured: i64) -> i64 {
    configured.max(2)
}
