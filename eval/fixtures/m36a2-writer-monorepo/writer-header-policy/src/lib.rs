mod policy;

pub use policy::HeaderError;

pub fn parse_forwarded_chain(
    header: Option<&str>,
    max_hops: usize,
) -> Result<Vec<String>, HeaderError> {
    let header = header.ok_or(HeaderError::Missing)?;
    let values: Vec<String> = header
        .split(',')
        .map(|value| value.trim().to_string())
        .collect();
    if values.is_empty() || values.len() > max_hops {
        return Err(HeaderError::TooManyHops);
    }
    Ok(values)
}
