use serde::Deserialize;
use serde::Serialize;

use super::error::Error;
use super::error::Result;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    /// Command may run without further approval.
    Allow,
    /// Request explicit user approval when the canonical run is not auto-approved.
    Prompt,
    /// Command is blocked without further consideration.
    Forbidden,
}

impl Decision {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "allow" => Ok(Self::Allow),
            "prompt" => Ok(Self::Prompt),
            "forbidden" => Ok(Self::Forbidden),
            other => Err(Error::InvalidDecision(other.to_string())),
        }
    }
}
