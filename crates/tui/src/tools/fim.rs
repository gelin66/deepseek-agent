//! FIM (Fill-in-the-Middle) edit tool.
//!
//! Reads a file, finds `prefix_anchor` and `suffix_anchor`, calls the
//! DeepSeek `/beta/completions` FIM endpoint, and writes the generated
//! middle content back into the file.

use std::fs;

use async_trait::async_trait;
use serde_json::{Value, json};
use thiserror::Error;

use crate::client::DeepSeekClient;

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
    optional_u64, required_str,
};

/// Result of a FIM edit operation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FimEditResult {
    pub success: bool,
    pub path: String,
    pub generated_text: String,
    pub prefix_end: usize,
    pub suffix_start: usize,
    pub message: String,
}

/// Tool for performing Fill-in-the-Middle edits via the DeepSeek FIM API.
pub struct FimEditTool {
    pub client: Option<DeepSeekClient>,
}

impl FimEditTool {
    #[must_use]
    pub fn new(client: Option<DeepSeekClient>) -> Self {
        Self { client }
    }
}

// === Errors ===

#[derive(Debug, Error)]
enum FimError {
    #[error("Prefix anchor not found in file: '{0}'")]
    PrefixNotFound(String),
    #[error("Suffix anchor not found after prefix anchor: '{0}'")]
    SuffixNotFound(String),
    #[error("Prefix and suffix anchors overlap (suffix starts at {0}, prefix ends at {1})")]
    AnchorsOverlap(usize, usize),
    #[error("FIM API call failed: {0}")]
    ApiFailed(String),
}

#[async_trait]
impl ToolSpec for FimEditTool {
    fn name(&self) -> &'static str {
        "fim_edit"
    }

    fn description(&self) -> &'static str {
        "Edit a file using Fill-in-the-Middle (FIM) completion. Provide a file path, \
         prefix_anchor (text that appears before the section to replace), and \
         suffix_anchor (text that appears after the section to replace). The tool \
         calls DeepSeek's FIM endpoint to generate replacement content."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit (relative to workspace)"
                },
                "prefix_anchor": {
                    "type": "string",
                    "description": "Text anchor marking the end of the prefix. Everything up to and including this anchor is kept as-is before the generated middle."
                },
                "suffix_anchor": {
                    "type": "string",
                    "description": "Text anchor marking the start of the suffix. Everything from this anchor onward is kept as-is after the generated middle."
                },
                "max_tokens": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 4096,
                    "description": "Maximum tokens to generate (default: 1024; DeepSeek FIM limit: 4096)"
                }
            },
            "required": ["path", "prefix_anchor", "suffix_anchor"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ReadOnly,
            ToolCapability::WritesFiles,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Suggest
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError> {
        let path = required_str(&input, "path")?;
        let prefix_anchor = required_str(&input, "prefix_anchor")?;
        let suffix_anchor = required_str(&input, "suffix_anchor")?;
        let max_tokens = optional_u64(&input, "max_tokens", 1024);
        if !(1..=4096).contains(&max_tokens) {
            return Err(ToolError::invalid_input(
                "max_tokens must be between 1 and 4096 for DeepSeek FIM",
            ));
        }

        // 1. Read the file
        let resolved = context.resolve_path(path)?;
        let content = fs::read_to_string(&resolved).map_err(|e| {
            ToolError::execution_failed(format!("Failed to read {}: {}", resolved.display(), e))
        })?;

        // 2. Find prefix anchor
        let prefix_pos = content.find(prefix_anchor).ok_or_else(|| {
            ToolError::execution_failed(
                FimError::PrefixNotFound(prefix_anchor.to_string()).to_string(),
            )
        })?;
        let prefix_end = prefix_pos + prefix_anchor.len();

        // 3. Find suffix anchor (after prefix anchor)
        let suffix_pos = content[prefix_end..].find(suffix_anchor).ok_or_else(|| {
            ToolError::execution_failed(
                FimError::SuffixNotFound(suffix_anchor.to_string()).to_string(),
            )
        })?;
        let suffix_start = prefix_end + suffix_pos;

        // 4. Validate anchors don't overlap
        if suffix_start < prefix_end {
            return Err(ToolError::execution_failed(
                FimError::AnchorsOverlap(suffix_start, prefix_end).to_string(),
            ));
        }

        // 5. Extract prefix and suffix for the FIM API
        let fim_prompt = content[..prefix_end].to_string();
        let fim_suffix = content[suffix_start..].to_string();

        // 6. Call FIM API
        let generated_text = match self.client.as_ref() {
            Some(client) => client
                .fim_completion(&fim_prompt, &fim_suffix, max_tokens as u32)
                .await
                .map_err(|e| {
                    ToolError::execution_failed(FimError::ApiFailed(e.to_string()).to_string())
                })?,
            None => {
                return Err(ToolError::execution_failed(
                    "FIM API client not available".to_string(),
                ));
            }
        };

        // The model call can take long enough for the user, a watcher, or a
        // sibling agent to edit the file. Never overwrite those newer bytes
        // with a completion based on the stale pre-request snapshot.
        let latest_content = fs::read_to_string(&resolved).map_err(|e| {
            ToolError::execution_failed(format!(
                "Failed to re-read {} before applying FIM edit: {}",
                resolved.display(),
                e
            ))
        })?;
        if latest_content != content {
            return Err(ToolError::execution_failed(
                "File changed while DeepSeek was generating the FIM edit; no changes were applied. Re-read the file and retry with fresh anchors."
                    .to_string(),
            ));
        }

        // 7. Build the new content and write it back
        let generated_len = generated_text.len();
        let new_content = format!("{fim_prompt}{generated_text}{fim_suffix}");
        crate::utils::write_atomic(&resolved, new_content.as_bytes()).map_err(|e| {
            ToolError::execution_failed(format!("Failed to write {}: {}", resolved.display(), e))
        })?;

        let result = FimEditResult {
            success: true,
            path: path.to_string(),
            generated_text,
            prefix_end,
            suffix_start,
            message: format!(
                "FIM edit applied to `{path}`. Generated {generated_len} chars between prefix_anchor end (byte {prefix_end}) and suffix_anchor start (byte {suffix_start}).",
            ),
        };

        ToolOutcome::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ProviderConfig, ProvidersConfig};
    use tempfile::tempdir;

    fn unsupported_fim_client() -> DeepSeekClient {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let providers = ProvidersConfig {
            zai: ProviderConfig {
                api_key: Some("zai-test".to_string()),
                base_url: Some("https://api.z.ai/api/coding/paas/v4".to_string()),
                ..ProviderConfig::default()
            },
            ..ProvidersConfig::default()
        };
        DeepSeekClient::new(&Config {
            provider: Some("zai".to_string()),
            providers: Some(providers),
            ..Config::default()
        })
        .expect("test client")
    }

    #[tokio::test]
    async fn fim_preflight_failure_never_modifies_the_file() {
        let workspace = tempdir().expect("workspace");
        let path = workspace.path().join("sample.rs");
        let original = "fn main() {\n    old();\n}\n";
        fs::write(&path, original).expect("seed file");
        let tool = FimEditTool::new(Some(unsupported_fim_client()));
        let context = ToolContext::new(workspace.path());

        let error = tool
            .execute(
                json!({
                    "path": "sample.rs",
                    "prefix_anchor": "fn main() {\n",
                    "suffix_anchor": "}\n",
                    "max_tokens": 16
                }),
                &context,
            )
            .await
            .expect_err("unsupported provider must fail before HTTP");

        assert!(error.to_string().contains("official DeepSeek"), "{error}");
        assert_eq!(fs::read_to_string(path).expect("read file"), original);
    }
}
