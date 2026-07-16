//! TUI composition for optional external shell sandboxes.

mod opensandbox;

use anyhow::Result;
use codewhale_tools::sandbox::backend::{SandboxBackend, SandboxKind};

use crate::config::Config;

/// Build the configured external sandbox transport.
pub fn create_backend(config: &Config) -> Result<Option<Box<dyn SandboxBackend>>> {
    let kind = config
        .sandbox_backend
        .as_deref()
        .and_then(SandboxKind::parse)
        .unwrap_or(SandboxKind::None);

    match kind {
        SandboxKind::None => Ok(None),
        SandboxKind::OpenSandbox => {
            let base_url = config
                .sandbox_url
                .clone()
                .unwrap_or_else(|| "http://localhost:8080".to_string());
            let backend =
                opensandbox::OpenSandboxBackend::new(base_url, config.sandbox_api_key.clone(), 30)?;
            Ok(Some(Box::new(backend)))
        }
    }
}
