//! Production adapter between the UI-independent Agent runtime and the
//! existing first-party tool implementations.
//!
//! This module is deliberately a conversion boundary. It does not own HTTP,
//! model protocol, tool implementations, or completion decisions.

use std::sync::Arc;

use async_trait::async_trait;
use codewhale_runtime::{
    CancellationToken, ToolDefinition, ToolExecutionError, ToolExecutor, ToolInvocation,
    ToolOperationStatus, ToolOutcome, ToolRetryDisposition, ToolSideEffectStatus,
};
use serde_json::Value;
use tokio_util::sync::CancellationToken as TokioCancellationToken;

use crate::tools::apply_patch::ApplyPatchTool;
use crate::tools::file::{EditFileTool, ListDirTool, ReadFileTool};
use crate::tools::file_search::FileSearchTool;
use crate::tools::git::{GitDiffTool, GitStatusTool};
use crate::tools::search::GrepFilesTool;
use crate::tools::shell::ExecShellTool;
use crate::tools::spec::{ToolContext, ToolError};
use crate::tools::test_runner::RunTestsTool;
use crate::tools::verifier::RunVerifiersTool;
use crate::tools::{ToolRegistry, ToolRegistryBuilder};

const MINIMAL_PRODUCTION_TOOL_NAMES: [&str; 11] = [
    "apply_patch",
    "edit_file",
    "exec_shell",
    "file_search",
    "git_diff",
    "git_status",
    "grep_files",
    "list_dir",
    "read_file",
    "run_tests",
    "run_verifiers",
];

fn production_tool_description(name: &str) -> &'static str {
    match name {
        "apply_patch" => {
            "用 unified diff 或完整文件内容原子修改一个或多个工作区文件；patch 与 changes 二选一，适合结构性、多处或跨文件变更。"
        }
        "edit_file" => {
            "在已用 read_file 读取的单个文件中执行一次精确搜索替换；适合范围明确的局部修改，匹配失败时会自动进行有限回退。"
        }
        "exec_shell" => {
            "在工作区中同步执行一条有界 shell 命令并返回退出状态与输出；优先使用更专用的文件、搜索、Git 或验证工具。"
        }
        "file_search" => {
            "按文件名或路径片段模糊查找工作区文件，可按扩展名和排除模式过滤并限制结果数量。"
        }
        "git_diff" => "读取工作区未提交或已暂存的 Git 差异，可限定路径和上下文行数。",
        "git_status" => "读取工作区的 Git 分支与文件状态，可限定到工作区内的路径。",
        "grep_files" => {
            "用正则表达式搜索工作区文本，可限定路径、包含或排除模式，并返回匹配行及上下文。"
        }
        "list_dir" => "列出工作区内指定目录的直接子项；path 省略时读取工作区根目录。",
        "read_file" => {
            "读取工作区内的 UTF-8 文本、PDF 或可 OCR 图像；大文件用 start_line/max_lines 分段，PDF 用 pages 指定页码。"
        }
        "run_tests" => {
            "在工作区根目录运行 cargo test，可传入额外参数或启用全部 feature，并返回确定性测试结果。"
        }
        "run_verifiers" => {
            "同步运行与项目类型匹配的确定性验证，可选择快速或完整等级，也可提供明确的程序与参数。"
        }
        _ => unreachable!("非生产工具进入固定 AgentRuntime 目录：{name}"),
    }
}

fn remove_schema_descriptions(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                remove_schema_descriptions(value);
            }
        }
        Value::Object(object) => {
            object.remove("description");
            for value in object.values_mut() {
                remove_schema_descriptions(value);
            }
        }
        _ => {}
    }
}

fn production_input_schema(name: &str, mut schema: Value) -> Value {
    remove_schema_descriptions(&mut schema);
    let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) else {
        return schema;
    };
    match name {
        // These fields only support legacy TUI execution paths. Hiding them
        // keeps the production AgentRuntime catalog convergent: every exposed
        // invocation has a result the same model turn can observe.
        "edit_file" => {
            properties.remove("fuzz");
        }
        "exec_shell" => {
            properties.remove("background");
            properties.remove("interactive");
            properties.remove("tty");
            properties.remove("combined_output");
        }
        "run_verifiers" => {
            properties.remove("background");
        }
        _ => {}
    }
    schema
}

/// The fixed first production tool surface for `codewhale exec`.
///
/// Runtime owns allow/deny policy. This adapter exposes only the audited
/// eleven-tool catalog and performs exact-name dispatch, so aliases and old
/// workflow/critic/sub-agent surfaces cannot leak into the new Runtime.
pub(crate) struct ProductionToolExecutor {
    registry: Arc<ToolRegistry>,
    definitions: Vec<ToolDefinition>,
}

impl ProductionToolExecutor {
    #[must_use]
    pub(crate) fn new(context: ToolContext) -> Self {
        let registry = ToolRegistryBuilder::new()
            .with_tool(Arc::new(ReadFileTool))
            .with_tool(Arc::new(ListDirTool))
            .with_tool(Arc::new(GrepFilesTool))
            .with_tool(Arc::new(FileSearchTool))
            .with_tool(Arc::new(ApplyPatchTool))
            .with_tool(Arc::new(EditFileTool))
            .with_tool(Arc::new(ExecShellTool))
            .with_tool(Arc::new(RunTestsTool))
            .with_tool(Arc::new(RunVerifiersTool))
            .with_tool(Arc::new(GitStatusTool))
            .with_tool(Arc::new(GitDiffTool))
            .build(context);
        Self::from_registry(registry)
    }

    fn from_registry(registry: ToolRegistry) -> Self {
        let mut definitions: Vec<_> = registry
            .to_api_tools()
            .into_iter()
            .map(|tool| ToolDefinition {
                description: production_tool_description(&tool.name).to_owned(),
                input_schema: production_input_schema(&tool.name, tool.input_schema),
                name: tool.name,
            })
            .collect();
        definitions.sort_by(|left, right| left.name.cmp(&right.name));
        Self {
            registry: Arc::new(registry),
            definitions,
        }
    }

    fn unavailable(name: &str) -> ToolOutcome {
        ToolOutcome::rejected(
            format!("tool_not_available：工具 '{name}' 未在生产 AgentRuntime 工具目录中提供"),
            ToolRetryDisposition::NotRetryable,
        )
    }

    fn tool_error_outcome(error: ToolError) -> ToolOutcome {
        match &error {
            ToolError::InvalidInput { .. }
            | ToolError::MissingField { .. }
            | ToolError::PathEscape { .. } => {
                ToolOutcome::rejected(error.to_string(), ToolRetryDisposition::AfterCorrection)
            }
            ToolError::NotAvailable { .. } | ToolError::PermissionDenied { .. } => {
                ToolOutcome::rejected(error.to_string(), ToolRetryDisposition::NotRetryable)
            }
            ToolError::Timeout { .. } => {
                let mut outcome = ToolOutcome::error(error.to_string());
                outcome.operation = ToolOperationStatus::Indeterminate;
                outcome.retry = ToolRetryDisposition::Unsafe;
                outcome
            }
            ToolError::ExecutionFailed { .. } => ToolOutcome::error(error.to_string()),
        }
    }
}

#[async_trait]
impl ToolExecutor for ProductionToolExecutor {
    fn definitions(&self) -> Vec<ToolDefinition> {
        self.definitions.clone()
    }

    async fn execute(
        &self,
        invocation: ToolInvocation,
        cancellation: CancellationToken,
    ) -> Result<ToolOutcome, ToolExecutionError> {
        if !MINIMAL_PRODUCTION_TOOL_NAMES.contains(&invocation.name.as_str())
            || !self.registry.contains(&invocation.name)
        {
            return Ok(Self::unavailable(&invocation.name));
        }

        let Some(input) = invocation.arguments.parsed else {
            return Ok(ToolOutcome::rejected(
                format!(
                    "invalid_arguments：工具 '{}' 的 JSON 参数格式错误：{}",
                    invocation.name, invocation.arguments.raw
                ),
                ToolRetryDisposition::AfterCorrection,
            ));
        };

        if cancellation.is_cancelled() {
            let mut outcome = ToolOutcome::error(format!(
                "tool_cancelled：工具 '{}' 在执行前已取消",
                invocation.name
            ));
            outcome.operation = ToolOperationStatus::Cancelled;
            outcome.side_effect = ToolSideEffectStatus::NotApplied;
            outcome.retry = ToolRetryDisposition::Safe;
            return Ok(outcome);
        }

        let tool_cancellation = TokioCancellationToken::new();
        let mut context = self.registry.context().clone();
        context.set_invocation_cancellation(tool_cancellation.clone());
        let execution =
            self.registry
                .execute_full_with_context(&invocation.name, input, Some(&context));
        tokio::pin!(execution);

        tokio::select! {
            result = &mut execution => Ok(match result {
                Ok(outcome) => outcome,
                Err(error) => Self::tool_error_outcome(error),
            }),
            () = cancellation.cancelled() => {
                tool_cancellation.cancel();
                match tokio::time::timeout(std::time::Duration::from_secs(5), &mut execution).await {
                    Ok(Ok(outcome)) => Ok(outcome),
                    Ok(Err(error)) => Ok(Self::tool_error_outcome(error)),
                    Err(_) => {
                        let mut outcome = ToolOutcome::recovery_ambiguous(format!(
                            "tool_cancel_timeout：工具 '{}' 收到取消后未在 5 秒内停止",
                            invocation.name
                        ));
                        outcome.operation = ToolOperationStatus::Cancelled;
                        Ok(outcome)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;
    use crate::tools::spec::{ToolCapability, ToolSpec};
    use codewhale_runtime::{RunId, ToolArguments, ToolInvocationStatus, ToolTransportStatus};

    struct TypedListDirTool;

    #[async_trait]
    impl ToolSpec for TypedListDirTool {
        fn name(&self) -> &'static str {
            "list_dir"
        }

        fn description(&self) -> &'static str {
            "typed outcome passthrough fixture"
        }

        fn input_schema(&self) -> Value {
            json!({"type": "object", "additionalProperties": false})
        }

        fn capabilities(&self) -> Vec<ToolCapability> {
            vec![ToolCapability::ReadOnly]
        }

        async fn execute(
            &self,
            _input: Value,
            _context: &ToolContext,
        ) -> Result<ToolOutcome, ToolError> {
            let mut outcome = ToolOutcome::success("typed");
            outcome.evidence = codewhale_runtime::ToolEvidence {
                status: codewhale_runtime::ToolEvidenceStatus::Produced,
                references: vec!["evidence-1".to_owned()],
            };
            outcome.workspace_revision = Some("revision-1".to_owned());
            outcome.metadata = Some(json!({"fixture": true}));
            Ok(outcome)
        }
    }

    #[tokio::test]
    async fn production_tools_are_exact_and_malformed_or_unknown_calls_fail_normally() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("visible.txt"), "fixture").unwrap();
        let executor = ProductionToolExecutor::new(ToolContext::new(temp.path()));
        let definitions = executor.definitions();
        let names: Vec<_> = definitions
            .iter()
            .map(|definition| definition.name.as_str())
            .collect();
        assert_eq!(names, MINIMAL_PRODUCTION_TOOL_NAMES);
        assert!(definitions.iter().all(|definition| {
            definition
                .description
                .chars()
                .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))
        }));

        let serialized = serde_json::to_string(&definitions).unwrap();
        for stale in [
            "backward compatibility",
            "write_file",
            "task_shell_start",
            "exec_shell_wait",
            "task_shell_wait",
            "task/status",
            "Find files by name",
            "Execute a shell command",
            "Optional extra arguments",
        ] {
            assert!(
                !serialized.contains(stale),
                "生产工具目录泄漏旧说明或不存在的工具 {stale}: {serialized}"
            );
        }
        let schema = |name: &str| {
            &definitions
                .iter()
                .find(|definition| definition.name == name)
                .unwrap()
                .input_schema
        };
        assert!(schema("edit_file").pointer("/properties/fuzz").is_none());
        for hidden in ["background", "interactive", "tty", "combined_output"] {
            assert!(
                schema("exec_shell")
                    .pointer(&format!("/properties/{hidden}"))
                    .is_none(),
                "exec_shell 暴露了未经本切片验证的字段 {hidden}"
            );
        }
        assert!(
            schema("run_verifiers")
                .pointer("/properties/background")
                .is_none()
        );

        let listed = executor
            .execute(
                ToolInvocation {
                    run_id: RunId::from("run-1"),
                    call_id: "list".to_owned(),
                    name: "list_dir".to_owned(),
                    arguments: ToolArguments::from_value(json!({"path":"."})),
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(listed.is_success());
        assert_eq!(listed.invocation, ToolInvocationStatus::Accepted);
        assert_eq!(listed.transport, ToolTransportStatus::Succeeded);
        assert_eq!(listed.operation, ToolOperationStatus::Succeeded);
        let entries: Value = serde_json::from_str(&listed.content).unwrap();
        assert!(
            entries
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| { entry["name"] == "visible.txt" && entry["is_dir"] == false })
        );

        let read = executor
            .execute(
                ToolInvocation {
                    run_id: RunId::from("run-1"),
                    call_id: "read".to_owned(),
                    name: "read_file".to_owned(),
                    arguments: ToolArguments::from_value(json!({"path": "visible.txt"})),
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(read.is_success());
        assert_eq!(read.content, "fixture");
        assert_eq!(read.invocation, ToolInvocationStatus::Accepted);
        assert_eq!(read.transport, ToolTransportStatus::Succeeded);
        assert_eq!(read.operation, ToolOperationStatus::Succeeded);

        let patched = executor
            .execute(
                ToolInvocation {
                    run_id: RunId::from("run-1"),
                    call_id: "patch".to_owned(),
                    name: "apply_patch".to_owned(),
                    arguments: ToolArguments::from_value(json!({
                        "path": "visible.txt",
                        "patch": "@@ -1 +1 @@\n-fixture\n+patched\n"
                    })),
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(patched.is_success());
        assert_eq!(patched.invocation, ToolInvocationStatus::Accepted);
        assert_eq!(patched.transport, ToolTransportStatus::Succeeded);
        assert_eq!(patched.operation, ToolOperationStatus::Succeeded);
        assert_eq!(patched.side_effect, ToolSideEffectStatus::Applied);
        assert_eq!(
            std::fs::read(temp.path().join("visible.txt")).unwrap(),
            b"patched"
        );

        let reread = executor
            .execute(
                ToolInvocation {
                    run_id: RunId::from("run-1"),
                    call_id: "reread".to_owned(),
                    name: "read_file".to_owned(),
                    arguments: ToolArguments::from_value(json!({"path": "visible.txt"})),
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(reread.is_success());
        assert_eq!(reread.content, "patched");

        let edited = executor
            .execute(
                ToolInvocation {
                    run_id: RunId::from("run-1"),
                    call_id: "edit".to_owned(),
                    name: "edit_file".to_owned(),
                    arguments: ToolArguments::from_value(json!({
                        "path": "visible.txt",
                        "search": "patched",
                        "replace": "edited"
                    })),
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(edited.is_success());
        assert_eq!(edited.invocation, ToolInvocationStatus::Accepted);
        assert_eq!(edited.transport, ToolTransportStatus::Succeeded);
        assert_eq!(edited.operation, ToolOperationStatus::Succeeded);
        assert_eq!(edited.side_effect, ToolSideEffectStatus::Applied);
        assert_eq!(
            std::fs::read(temp.path().join("visible.txt")).unwrap(),
            b"edited"
        );

        let malformed = executor
            .execute(
                ToolInvocation {
                    run_id: RunId::from("run-1"),
                    call_id: "bad".to_owned(),
                    name: "read_file".to_owned(),
                    arguments: ToolArguments::parse("{not-json"),
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(!malformed.is_success());
        assert_eq!(malformed.invocation, ToolInvocationStatus::Rejected);
        assert_eq!(malformed.transport, ToolTransportStatus::NotStarted);
        assert_eq!(malformed.operation, ToolOperationStatus::NotStarted);
        assert_eq!(malformed.retry, ToolRetryDisposition::AfterCorrection);
        assert!(malformed.content.contains("invalid_arguments"));
        assert!(malformed.content.contains("JSON 参数格式错误"));
        assert!(malformed.content.contains("{not-json"));

        let empty = executor
            .execute(
                ToolInvocation {
                    run_id: RunId::from("run-1"),
                    call_id: "empty".to_owned(),
                    name: "read_file".to_owned(),
                    arguments: ToolArguments::parse(""),
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(!empty.is_success());
        assert!(empty.content.contains("invalid_arguments"));

        let unknown = executor
            .execute(
                ToolInvocation {
                    run_id: RunId::from("run-1"),
                    call_id: "unknown".to_owned(),
                    name: "verify".to_owned(),
                    arguments: ToolArguments::from_value(json!({})),
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();
        assert!(!unknown.is_success());
        assert_eq!(unknown.invocation, ToolInvocationStatus::Rejected);
        assert_eq!(unknown.transport, ToolTransportStatus::NotStarted);
        assert_eq!(unknown.retry, ToolRetryDisposition::NotRetryable);
        assert!(unknown.content.contains("verify"));
        assert!(unknown.content.contains("tool_not_available"));
        assert!(
            unknown
                .content
                .contains("未在生产 AgentRuntime 工具目录中提供")
        );
    }

    #[tokio::test]
    async fn production_executor_preserves_the_canonical_tool_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let context = ToolContext::new(temp.path());
        let mut registry = ToolRegistry::new(context);
        registry.register(Arc::new(TypedListDirTool));
        let executor = ProductionToolExecutor::from_registry(registry);

        let outcome = executor
            .execute(
                ToolInvocation {
                    run_id: RunId::from("run-typed"),
                    call_id: "typed".to_owned(),
                    name: "list_dir".to_owned(),
                    arguments: ToolArguments::from_value(json!({})),
                },
                CancellationToken::default(),
            )
            .await
            .unwrap();

        assert_eq!(
            outcome.evidence.status,
            codewhale_runtime::ToolEvidenceStatus::Produced
        );
        assert_eq!(outcome.evidence.references, vec!["evidence-1".to_owned()]);
        assert_eq!(outcome.workspace_revision.as_deref(), Some("revision-1"));
        assert_eq!(outcome.metadata, Some(json!({"fixture": true})));
    }

    #[test]
    fn production_executor_maps_typed_tool_errors_without_string_classification() {
        let invalid = ProductionToolExecutor::tool_error_outcome(ToolError::missing_field("path"));
        assert_eq!(invalid.invocation, ToolInvocationStatus::Rejected);
        assert_eq!(invalid.transport, ToolTransportStatus::NotStarted);
        assert_eq!(invalid.operation, ToolOperationStatus::NotStarted);
        assert_eq!(invalid.retry, ToolRetryDisposition::AfterCorrection);

        let unavailable = ProductionToolExecutor::tool_error_outcome(ToolError::not_available(
            "fixture dependency",
        ));
        assert_eq!(unavailable.invocation, ToolInvocationStatus::Rejected);
        assert_eq!(unavailable.retry, ToolRetryDisposition::NotRetryable);

        let timeout = ProductionToolExecutor::tool_error_outcome(ToolError::Timeout { seconds: 1 });
        assert_eq!(timeout.invocation, ToolInvocationStatus::Accepted);
        assert_eq!(timeout.transport, ToolTransportStatus::Succeeded);
        assert_eq!(timeout.operation, ToolOperationStatus::Indeterminate);
        assert_eq!(timeout.retry, ToolRetryDisposition::Unsafe);
    }

    #[tokio::test]
    async fn cancelled_tool_never_starts() {
        let temp = tempfile::tempdir().unwrap();
        let executor = ProductionToolExecutor::new(ToolContext::new(temp.path()));
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let result = executor
            .execute(
                ToolInvocation {
                    run_id: RunId::from("run-1"),
                    call_id: "cancelled".to_owned(),
                    name: "list_dir".to_owned(),
                    arguments: ToolArguments::from_value(json!({"path":"."})),
                },
                cancellation,
            )
            .await
            .unwrap();
        assert!(!result.is_success());
        assert_eq!(result.transport, ToolTransportStatus::Succeeded);
        assert_eq!(result.operation, ToolOperationStatus::Cancelled);
        assert_eq!(result.side_effect, ToolSideEffectStatus::NotApplied);
        assert_eq!(result.retry, ToolRetryDisposition::Safe);
        assert!(result.content.contains("执行前已取消"));
    }
}
