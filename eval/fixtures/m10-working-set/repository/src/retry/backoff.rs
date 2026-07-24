pub fn bounded_retry_delay(attempt: u32, maximum_ms: u64) -> u64 {
    let exponential = 1_u64.checked_shl(attempt).unwrap_or(u64::MAX);
    exponential.saturating_mul(100).min(maximum_ms)
}
