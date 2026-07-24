pub fn classify_stream(bytes: &[u8]) -> Result<(), &'static str> {
    if bytes.ends_with(b"[DONE]") {
        Ok(())
    } else {
        Err("incomplete_stream")
    }
}
