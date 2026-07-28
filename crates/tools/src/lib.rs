use std::path::PathBuf;

pub use dse_protocol::agent_runtime::ToolOutcome;
use serde_json::Value;

mod application_probe;
mod apply_patch;
mod atomic_write;
pub mod child_env;
pub mod command_safety;
mod edit_file;
mod file_search;
mod git;
mod grep_files;
mod image_ocr;
mod list_dir;
mod load_skill;
mod production;
mod production_context;
mod read_file;
mod run_tests;
mod run_verifiers;
pub mod sandbox;
mod semantic_browser;
pub mod shell;
pub mod shell_dispatcher;
mod unified_diff;
mod verification_artifact;
mod web_fetch;

pub use application_probe::ApplicationProbeRecovery;
pub(crate) use application_probe::{
    APPLICATION_PROBE_VERIFIER_ID, execute_application_probe, recover_application_probe,
    resolve_application_probe_spec, validate_application_probe_spec,
};
pub(crate) use apply_patch::{execute_apply_patch, preflight_apply_patch};
pub use atomic_write::write_atomic;
pub(crate) use edit_file::execute_edit_file;
pub use file_search::execute_file_search;
pub use git::{execute_git_diff, execute_git_status};
pub use grep_files::execute_grep_files;
pub use image_ocr::{ocr_available, ocr_image_path, resolve_tesseract};
pub use list_dir::execute_list_dir;
pub(crate) use load_skill::{execute_load_skill, preflight_load_skill};
pub use production::{
    PRODUCTION_TOOL_NAMES, ProductionToolConfig, ProductionToolExecutionIdentity,
    ProductionToolExecutor, production_tool_definitions,
};
pub use production_context::ProductionToolContext;
pub use read_file::execute_read_file;
pub use run_tests::{CargoTestEvidence, RunTestsOutput};
pub(crate) use run_tests::{execute_run_tests, resolve_run_tests_spec};
pub use run_verifiers::{GateResult, GateStatus, RunVerifiersOutput, VerifierVerdict};
pub(crate) use run_verifiers::{execute_run_verifiers, resolve_run_verifiers_spec};
pub use semantic_browser::{
    BrowserCancellationToken, BrowserClickRequest, BrowserNavigateRequest, SemanticBrowserHarness,
    SystemSemanticBrowserHarness,
};
pub use unified_diff::make_unified_diff;
pub use verification_artifact::{
    attach_verifier_observation, capture_workspace_revision, reject_verification_artifact,
};
pub use web_fetch::{
    SystemWebFetchNetwork, WebFetchHttpResponse, WebFetchNetwork, WebFetchNetworkError,
};

#[cfg(test)]
mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    pub fn lock_test_env() -> MutexGuard<'static, ()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Errors that can occur during tool execution.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolError {
    #[error("工具参数不符合 schema：{message}")]
    SchemaValidation { message: String },
    #[error("工具参数无效：{message}")]
    InvalidInput { message: String },
    #[error("工具参数无效：缺少必填字段 '{field}'")]
    MissingField { field: String },
    #[error("路径解析失败：'{}' 超出工作区", path.display())]
    PathEscape { path: PathBuf },
    #[error("工具执行失败：{message}")]
    ExecutionFailed { message: String },
    #[error("工具执行失败：操作在 {seconds} 秒后超时")]
    Timeout { seconds: u64 },
    #[error("找不到工具：{message}")]
    NotAvailable { message: String },
    #[error("工具执行未获授权：{message}")]
    PermissionDenied { message: String },
    #[error("工作区前置条件不满足：{message}")]
    WorkspacePrecondition { message: String },
    #[error("工作区在读取后发生变化：{message}")]
    StaleRead { message: String },
    #[error("编辑目标不唯一：{message}")]
    AmbiguousEdit { message: String },
    #[error("Patch 参数无效：{message}")]
    PatchParse { message: String },
}

impl ToolError {
    #[must_use]
    pub fn schema_validation(msg: impl Into<String>) -> Self {
        Self::SchemaValidation {
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::MissingField {
            field: field.into(),
        }
    }

    #[must_use]
    pub fn execution_failed(msg: impl Into<String>) -> Self {
        Self::ExecutionFailed {
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn path_escape(path: impl Into<PathBuf>) -> Self {
        Self::PathEscape { path: path.into() }
    }

    #[must_use]
    pub fn not_available(msg: impl Into<String>) -> Self {
        Self::NotAvailable {
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn permission_denied(msg: impl Into<String>) -> Self {
        Self::PermissionDenied {
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn workspace_precondition(msg: impl Into<String>) -> Self {
        Self::WorkspacePrecondition {
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn stale_read(msg: impl Into<String>) -> Self {
        Self::StaleRead {
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn ambiguous_edit(msg: impl Into<String>) -> Self {
        Self::AmbiguousEdit {
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn patch_parse(msg: impl Into<String>) -> Self {
        Self::PatchParse {
            message: msg.into(),
        }
    }
}

/// Helper to extract a required string field from JSON input.
pub fn required_str<'a>(input: &'a Value, field: &str) -> std::result::Result<&'a str, ToolError> {
    input.get(field).and_then(Value::as_str).ok_or_else(|| {
        // When the field is missing, list the fields the caller *did*
        // supply so the model can spot the mismatch without a retry.
        let provided: Vec<&str> = input
            .as_object()
            .map(|obj| obj.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();
        if provided.is_empty() {
            ToolError::missing_field(field)
        } else {
            let hint = format!(
                "缺少必填字段 '{field}'。已提供字段：{}",
                provided.join(", ")
            );
            ToolError::invalid_input(hint)
        }
    })
}

/// Helper to extract an optional string field from JSON input.
#[must_use]
pub fn optional_str<'a>(input: &'a Value, field: &str) -> Option<&'a str> {
    input.get(field).and_then(Value::as_str)
}

/// Helper to extract a required u64 field from JSON input.
pub fn required_u64(input: &Value, field: &str) -> std::result::Result<u64, ToolError> {
    input
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::missing_field(field))
}

/// Helper to extract an optional u64 field with default.
#[must_use]
pub fn optional_u64(input: &Value, field: &str, default: u64) -> u64 {
    input.get(field).and_then(Value::as_u64).unwrap_or(default)
}

/// Helper to extract an optional bool field with default.
#[must_use]
pub fn optional_bool(input: &Value, field: &str, default: bool) -> bool {
    input.get(field).and_then(Value::as_bool).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tool_outcome_success_sets_plain_content() {
        let content = "operation completed successfully";
        let result = ToolOutcome::success(content);

        assert!(result.is_success());
        assert_eq!(result.content, content);
        assert!(result.metadata.is_none());
    }

    #[test]
    fn tool_outcome_json_round_trips_content() {
        let result = ToolOutcome::json(&json!({"ok": true})).expect("json");
        assert!(result.is_success());
        let content: serde_json::Value =
            serde_json::from_str(&result.content).expect("content is valid json");
        assert_eq!(content, json!({"ok": true}));
    }

    #[test]
    fn helper_extractors_validate_shape() {
        let input = json!({"name": "demo", "count": 7, "enabled": true});
        assert_eq!(required_str(&input, "name").expect("name"), "demo");
        assert_eq!(optional_str(&input, "name"), Some("demo"));
        assert_eq!(optional_str(&input, "missing"), None);
        assert_eq!(optional_str(&input, "count"), None);
        assert_eq!(optional_str(&json!({"name": null}), "name"), None);
        assert_eq!(optional_u64(&input, "count", 0), 7);
        assert!(optional_bool(&input, "enabled", false));
        assert!(matches!(
            required_u64(&input, "name"),
            Err(ToolError::MissingField { .. })
        ));
    }

    #[test]
    fn required_u64_rejects_missing_or_non_integer_values() {
        assert!(matches!(
            required_u64(&json!({}), "count"),
            Err(ToolError::MissingField { .. })
        ));
        assert_eq!(required_u64(&json!({"count": 42}), "count").unwrap(), 42);
        assert_eq!(
            required_u64(&json!({"count": u64::MAX}), "count").unwrap(),
            u64::MAX
        );

        for value in [json!(-1), json!(2.5), json!("42")] {
            assert!(matches!(
                required_u64(&json!({"count": value}), "count"),
                Err(ToolError::MissingField { .. })
            ));
        }
    }

    #[test]
    fn required_str_reports_provided_fields_on_missing_required_field() {
        let input = json!({"path": "src/lib.rs", "content": "new body"});
        let err = required_str(&input, "replace").expect_err("replace is missing");
        let message = err.to_string();
        assert!(message.contains("缺少必填字段 'replace'"));
        assert!(message.contains("已提供字段："));
        assert!(message.contains("path"));
        assert!(message.contains("content"));
    }

    #[test]
    fn tool_error_display_is_chinese_and_keeps_field_name() {
        let err = ToolError::missing_field("path");
        assert_eq!(err.to_string(), "工具参数无效：缺少必填字段 'path'");
    }

    #[test]
    fn tool_error_missing_field_constructor() {
        let err = ToolError::missing_field("my_field");
        assert!(matches!(err, ToolError::MissingField { field } if field == "my_field"));
    }

    #[test]
    fn tool_error_not_available_displays_reason() {
        let err = ToolError::not_available("custom tool not found");

        assert!(matches!(err, ToolError::NotAvailable { .. }));
        assert_eq!(err.to_string(), "找不到工具：custom tool not found");
    }

    #[test]
    fn tool_error_permission_denied_displays_reason() {
        let err = ToolError::permission_denied("unauthorized user");

        assert!(matches!(err, ToolError::PermissionDenied { .. }));
        assert_eq!(err.to_string(), "工具执行未获授权：unauthorized user");
    }

    #[test]
    fn tool_error_execution_failed_displays_reason() {
        let err = ToolError::execution_failed("process crashed");

        assert!(
            matches!(err, ToolError::ExecutionFailed { ref message } if message == "process crashed")
        );
        assert_eq!(err.to_string(), "工具执行失败：process crashed");
    }

    #[test]
    fn tool_error_invalid_input_creates_correct_variant() {
        let err = ToolError::invalid_input("test invalid message");
        match err {
            ToolError::InvalidInput { message } => {
                assert_eq!(message, "test invalid message");
            }
            _ => panic!("Expected ToolError::InvalidInput, got {err:?}"),
        }
    }

    #[test]
    fn tool_error_path_escape_display() {
        let path = std::path::PathBuf::from("../outside");
        let err = ToolError::path_escape(path);
        assert_eq!(err.to_string(), "路径解析失败：'../outside' 超出工作区");
    }
}
