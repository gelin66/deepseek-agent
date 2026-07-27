#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderError {
    Missing,
    InvalidLimit,
    InvalidMember,
    Duplicate,
    TooManyHops,
}

pub fn valid_member(value: &str) -> bool {
    !value.is_empty() && value.len() <= 63
}
