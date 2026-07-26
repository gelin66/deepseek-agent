pub fn canonical_service_name(value: &str) -> Result<String, &'static str> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("empty");
    }
    if !normalized
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("invalid");
    }
    Ok(normalized)
}
