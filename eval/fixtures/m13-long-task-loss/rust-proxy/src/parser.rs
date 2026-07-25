pub(crate) fn split_assignment(value: &str) -> Result<(&str, &str), &'static str> {
    let mut parts = value.split('=');
    let name = parts.next().unwrap_or_default();
    let content = parts.next().ok_or("missing value")?;
    if name.is_empty() || content.is_empty() || parts.next().is_some() {
        return Err("invalid assignment");
    }
    Ok((name, content))
}

pub(crate) fn normalize_endpoint(value: &str) -> Result<String, &'static str> {
    let value = value.trim().trim_end_matches('/');
    if value.starts_with("http://") || value.starts_with("https://") {
        Ok(value.to_owned())
    } else {
        Err("invalid endpoint")
    }
}
