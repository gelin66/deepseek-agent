pub fn normalize_endpoint(value: &str) -> Result<String, &'static str> {
    let value = value.trim();
    if value.is_empty() {
        return Err("empty endpoint");
    }
    Ok(value.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::normalize_endpoint;

    #[test]
    fn keeps_basic_https_endpoint() {
        assert_eq!(
            normalize_endpoint("https://api.example.test").unwrap(),
            "https://api.example.test"
        );
    }
}
