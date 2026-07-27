pub fn canonical_header_name(raw: &str) -> String {
    raw.trim().replace('_', "-").to_ascii_uppercase()
}
