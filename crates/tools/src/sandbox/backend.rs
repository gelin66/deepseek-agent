//! Pluggable external-sandbox execution port.
//!
//! External sandbox backends route shell command execution to a remote service
//! (e.g. Alibaba OpenSandbox) instead of spawning a local process. This is
//! complementary to the OS-level sandbox module (Seatbelt / Landlock / Windows)
//! — the external backend *replaces* local execution entirely when configured.

use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Output from a sandbox backend execution.
#[derive(Debug, Clone)]
pub struct SandboxOutput {
    /// Standard output from the command.
    pub stdout: String,
    /// Standard error from the command.
    pub stderr: String,
    /// Exit code (0 for success).
    pub exit_code: i32,
}

/// The kind of external sandbox backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxKind {
    /// No external sandbox — execute commands locally.
    None,
    /// Alibaba OpenSandbox remote execution.
    OpenSandbox,
}

impl SandboxKind {
    /// Parse a sandbox backend name from config (case-insensitive).
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "" => Some(Self::None),
            "opensandbox" | "open-sandbox" | "open_sandbox" => Some(Self::OpenSandbox),
            _ => None,
        }
    }

    /// Human-readable label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::OpenSandbox => "opensandbox",
        }
    }
}

/// Stable non-secret identity for resume/fingerprint decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxBackendIdentity {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    pub authentication_configured: bool,
}

impl SandboxBackendIdentity {
    /// Build an identity without exposing the endpoint or credential value.
    #[must_use]
    pub fn remote(
        kind: impl Into<String>,
        endpoint: &str,
        timeout_ms: u64,
        authentication_configured: bool,
    ) -> Self {
        let digest = Sha256::digest(endpoint.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Self {
            kind: kind.into(),
            endpoint_sha256: Some(format!("sha256:{digest}")),
            timeout_ms: Some(timeout_ms),
            authentication_configured,
        }
    }
}

/// Abstract interface for an external sandbox backend.
///
/// Implementations send commands to a remote execution environment and return
/// structured output. The trait is `Send + Sync` so it can be stored in an
/// `Arc` and shared across async tasks.
#[async_trait]
pub trait SandboxBackend: Send + Sync {
    /// Return stable non-secret execution identity. Implementations must not
    /// expose endpoint text, credentials or other secret-bearing headers.
    fn identity(&self) -> SandboxBackendIdentity;

    /// Execute a shell command and return its output.
    ///
    /// `cmd` is the full shell command string (e.g. `"ls -la"`).
    /// `env` contains additional environment variables to set.
    async fn exec(&self, cmd: &str, env: &HashMap<String, String>) -> Result<SandboxOutput>;
}
