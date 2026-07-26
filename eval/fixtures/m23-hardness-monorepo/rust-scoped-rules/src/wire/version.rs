pub fn normalize_wire_version(value: &str) -> Result<String, &'static str> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("empty version");
    }
    Ok(normalized)
}
