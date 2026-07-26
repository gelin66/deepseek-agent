#[derive(Default)]
pub struct NetstringDecoder {
    pending: Vec<u8>,
}

impl NetstringDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, &'static str> {
        let text = String::from_utf8(chunk.to_vec()).map_err(|_| "invalid netstring")?;
        Ok(vec![text])
    }

    pub fn finish(&mut self) -> Result<(), &'static str> {
        self.pending.clear();
        Ok(())
    }
}
