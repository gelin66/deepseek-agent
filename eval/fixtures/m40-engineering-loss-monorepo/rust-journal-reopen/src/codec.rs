pub fn encode(payload: &str) -> Vec<u8> {
    format!("{}:{}\n", payload.chars().count(), payload).into_bytes()
}

pub fn decode(line: &[u8]) -> Result<String, &'static str> {
    let text = std::str::from_utf8(line).map_err(|_| "journal_utf8")?;
    let (_, payload) = text.split_once(':').ok_or("journal_header")?;
    Ok(payload.to_owned())
}
