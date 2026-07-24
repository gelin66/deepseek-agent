use super::stream::classify_stream;

pub fn dispatch(bytes: &[u8]) -> Result<(), &'static str> {
    classify_stream(bytes)
}
