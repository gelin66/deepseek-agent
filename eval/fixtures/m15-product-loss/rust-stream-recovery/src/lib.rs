#[derive(Default)]
pub struct LineFramer {
    pending: Vec<u8>,
}

impl LineFramer {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, &'static str> {
        let text = String::from_utf8(chunk.to_vec()).map_err(|_| "invalid utf-8")?;
        Ok(text.lines().map(str::to_owned).collect())
    }

    pub fn finish(&mut self) -> Result<Vec<String>, &'static str> {
        self.pending.clear();
        Ok(Vec::new())
    }
}
