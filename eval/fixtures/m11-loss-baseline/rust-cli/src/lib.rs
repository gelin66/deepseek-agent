pub fn parse_bind_address(value: &str) -> Result<(String, u16), String> {
    let (host, port) = value
        .split_once(':')
        .ok_or_else(|| "expected host:port".to_owned())?;
    let port = port
        .parse::<u16>()
        .map_err(|_| "invalid port".to_owned())?;
    Ok((host.to_owned(), port))
}
