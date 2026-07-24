pub fn retry_delay_ms(attempt: u32, base_ms: u64, cap_ms: u64) -> Result<u64, String> {
    if attempt == 0 {
        return Err("attempts are one-based".to_owned());
    }
    Ok(base_ms.saturating_mul(1_u64 << attempt).min(cap_ms))
}
