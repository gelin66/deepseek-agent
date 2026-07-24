#[derive(Debug, PartialEq)]
pub struct RuntimePolicy {
    pub retries: u32,
}

pub fn parse_runtime_policy(value: Option<&str>) -> RuntimePolicy {
    let retries = value.unwrap_or("0").parse().unwrap_or(0);
    RuntimePolicy { retries }
}
