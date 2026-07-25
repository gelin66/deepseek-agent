use crate::parser::{normalize_endpoint, split_assignment};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyConfig {
    pub endpoint: String,
    pub timeout_ms: u32,
    pub headers: Vec<(String, String)>,
}

pub fn parse_proxy_args(arguments: &[&str]) -> Result<ProxyConfig, &'static str> {
    let mut endpoint = None;
    let mut timeout_ms = 5_000;
    let mut headers = Vec::new();
    for argument in arguments {
        if let Some(value) = argument.strip_prefix("--endpoint=") {
            endpoint = Some(normalize_endpoint(value)?);
        } else if let Some(value) = argument.strip_prefix("--timeout-ms=") {
            timeout_ms = value.parse().map_err(|_| "invalid timeout")?;
        } else if let Some(value) = argument.strip_prefix("--header=") {
            let (name, content) = split_assignment(value)?;
            headers.push((name.to_owned(), content.to_owned()));
        }
    }
    Ok(ProxyConfig {
        endpoint: endpoint.ok_or("missing endpoint")?,
        timeout_ms,
        headers,
    })
}
