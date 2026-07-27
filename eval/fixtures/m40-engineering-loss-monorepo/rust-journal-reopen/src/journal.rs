use crate::codec::{decode, encode};

pub fn append_record(buffer: &mut Vec<u8>, payload: &str) {
    buffer.extend(encode(payload));
}

pub fn reopen_records(buffer: &[u8]) -> Result<Vec<String>, &'static str> {
    buffer
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(decode)
        .collect()
}
