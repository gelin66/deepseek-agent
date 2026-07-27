pub fn canonical_header_name(raw: &str) -> Result<String, &'static str> {
    if raw.is_empty() {
        return Err("header_empty");
    }
    Ok(raw.to_ascii_lowercase())
}
