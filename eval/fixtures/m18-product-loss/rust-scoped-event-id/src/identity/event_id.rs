pub fn normalize_event_id(value: &str) -> Result<String, &'static str> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("empty event id");
    }
    Ok(normalized)
}
