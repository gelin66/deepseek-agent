//! Production adapters between the UI-independent Agent runtime and the
//! existing DeepSeek transport / first-party tool implementations.
//!
//! This module is deliberately a conversion boundary. It does not own HTTP,
//! SSE decoding, retry policy, request planning, tool implementations, or
//! completion decisions.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use codewhale_runtime::{
    ActorRequestAccounting, AgentActorKind, ApiSurface as RuntimeApiSurface, CancellationToken,
    ModelAccounting, ModelErrorCategory, ModelFinishReason, ModelOutput, ModelPort, ModelPortError,
    ModelRequest, ModelStream, ModelStreamEvent, ModelToolCall, PromptCacheControl, SurfaceUsage,
    SystemPrompt as RuntimeSystemPrompt, SystemPromptBlock as RuntimeSystemPromptBlock,
    ToolArguments, ToolDefinition, ToolExecutionError, ToolExecutor, ToolInvocation,
    ToolOperationStatus, ToolOutcome, ToolRetryDisposition, ToolSideEffectStatus,
    Usage as RuntimeUsage,
};
use futures_util::StreamExt;
use serde_json::Value;
use tokio_util::sync::CancellationToken as TokioCancellationToken;

use crate::client::DeepSeekClient;
use crate::client::deepseek::{ApiSurface, ChatPlanError};
use crate::client::request_budget::{
    ApiRequestActorSnapshot, ApiRequestBudgetSnapshot, ApiUsageSnapshot, SharedApiRequestBudget,
};
use crate::error_taxonomy::StreamError;
use crate::llm_client::{LlmError, StreamEventBox};
use crate::models::{
    ContentBlock, ContentBlockStart, Delta, MessageResponse, StreamEvent, SystemPrompt, Usage,
};
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

/// DeepSeek transport adapter for both root and child runs.
///
/// Every request is rebound to the same admission/accounting owner. The
/// request's canonical actor selects only the attribution view; it never
/// creates another budget or another transport implementation.
pub(crate) struct DeepSeekModelPort {
    client: DeepSeekClient,
    accounting: SharedApiRequestBudget,
}

impl DeepSeekModelPort {
    #[must_use]
    pub(crate) fn new(client: DeepSeekClient, accounting: SharedApiRequestBudget) -> Self {
        Self {
            client: client.with_transport_retries_disabled_for_runtime(),
            accounting,
        }
    }

    fn client_for(&self, actor: AgentActorKind) -> DeepSeekClient {
        let accounting = match actor {
            AgentActorKind::Root => self.accounting.clone(),
            AgentActorKind::Child => self.accounting.for_child(),
        };
        self.client.clone().with_api_request_budget(accounting)
    }
}

#[async_trait]
impl ModelPort for DeepSeekModelPort {
    async fn stream(&self, request: ModelRequest) -> Result<Box<dyn ModelStream>, ModelPortError> {
        let client = self.client_for(request.actor.kind);
        let plan = client
            .plan_runtime_chat(&request)
            .map_err(|error| model_port_error(error.into()))?
            .ok_or_else(|| {
                ModelPortError::new(
                    "deepseek_official_route_required",
                    ModelErrorCategory::Protocol,
                    "AgentRuntime requires the official DeepSeek Chat route",
                    false,
                )
            })?;

        if !request.streaming {
            let response = client
                .create_planned_deepseek_message(plan)
                .await
                .map_err(model_port_error)?;
            return Ok(Box::new(BufferedModelStream::from_response(response)?));
        }

        let source = client
            .handle_planned_chat_completion_stream(plan)
            .await
            .map_err(model_port_error)?;
        Ok(Box::new(DeepSeekStreamingAdapter::new(source)))
    }

    async fn accounting_snapshot(&self, seal: bool) -> Result<ModelAccounting, ModelPortError> {
        let (requests, actors, usage) = if seal {
            self.accounting.seal_and_accounting_snapshot()
        } else {
            self.accounting.accounting_snapshot()
        };
        Ok(runtime_accounting(requests, actors, usage))
    }
}

/// Projects the existing prompt builder's value into the runtime's canonical
/// prompt without flattening block order or losing its stable-prefix marker.
#[must_use]
pub(crate) fn canonical_system_prompt(prompt: &SystemPrompt) -> RuntimeSystemPrompt {
    match prompt {
        SystemPrompt::Text(text) => RuntimeSystemPrompt::from_text(text.clone()),
        SystemPrompt::Blocks(blocks) => RuntimeSystemPrompt {
            blocks: blocks
                .iter()
                .enumerate()
                .map(|(index, block)| RuntimeSystemPromptBlock {
                    text: block.text.clone(),
                    cache_control: if index == 0 || block.cache_control.is_some() {
                        PromptCacheControl::Stable
                    } else {
                        PromptCacheControl::Volatile
                    },
                })
                .collect(),
        },
    }
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

fn runtime_usage(usage: &Usage) -> RuntimeUsage {
    RuntimeUsage {
        input_tokens: u64::from(usage.input_tokens),
        output_tokens: u64::from(usage.output_tokens),
        cache_hit_tokens: u64::from(usage.prompt_cache_hit_tokens.unwrap_or(0)),
        cache_miss_tokens: u64::from(usage.prompt_cache_miss_tokens.unwrap_or(0)),
        cache_write_tokens: u64::from(usage.prompt_cache_write_tokens.unwrap_or(0)),
        reasoning_tokens: u64::from(usage.reasoning_tokens.unwrap_or(0)),
        reasoning_replay_tokens: u64::from(usage.reasoning_replay_tokens.unwrap_or(0)),
    }
}

fn finish_reason(reason: Option<&str>) -> Result<ModelFinishReason, ModelPortError> {
    match reason {
        Some("stop") | Some("end_turn") => Ok(ModelFinishReason::Stop),
        Some("tool_calls") | Some("tool_use") => Ok(ModelFinishReason::ToolCalls),
        Some("length") | Some("max_tokens") => Ok(ModelFinishReason::Length),
        Some("content_filter") => Ok(ModelFinishReason::ContentFilter),
        Some("insufficient_system_resource") => Ok(ModelFinishReason::InsufficientSystemResource),
        Some(other) => Err(ModelPortError::new(
            "deepseek_finish_reason_unknown",
            ModelErrorCategory::Protocol,
            format!("DeepSeek returned unsupported finish_reason '{other}'"),
            false,
        )),
        None => Err(ModelPortError::new(
            "deepseek_finish_reason_missing",
            ModelErrorCategory::Protocol,
            "DeepSeek response completed without finish_reason",
            false,
        )),
    }
}

fn response_output(response: MessageResponse) -> Result<ModelOutput, ModelPortError> {
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    for block in response.content {
        match block {
            ContentBlock::Text { text, .. } => content.push_str(&text),
            ContentBlock::Thinking { thinking, .. } => reasoning.push_str(&thinking),
            ContentBlock::ToolUse {
                id,
                name,
                input,
                raw_arguments,
                ..
            } => tool_calls.push(ModelToolCall {
                id,
                name,
                arguments: raw_arguments
                    .map_or_else(|| ToolArguments::from_value(input), ToolArguments::parse),
            }),
            ContentBlock::ImageUrl { .. }
            | ContentBlock::ToolResult { .. }
            | ContentBlock::ServerToolUse { .. }
            | ContentBlock::ToolSearchToolResult { .. }
            | ContentBlock::CodeExecutionToolResult { .. } => {}
        }
    }
    Ok(ModelOutput {
        content,
        reasoning_content: (!reasoning.is_empty()).then_some(reasoning),
        tool_calls,
        finish_reason: finish_reason(response.stop_reason.as_deref())?,
        usage: runtime_usage(&response.usage),
    })
}

struct BufferedModelStream {
    events: VecDeque<Result<ModelStreamEvent, ModelPortError>>,
}

impl BufferedModelStream {
    fn from_response(response: MessageResponse) -> Result<Self, ModelPortError> {
        let output = response_output(response)?;
        Ok(Self {
            events: VecDeque::from([Ok(ModelStreamEvent::Completed { output })]),
        })
    }
}

#[async_trait]
impl ModelStream for BufferedModelStream {
    async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>> {
        self.events.pop_front()
    }
}

#[derive(Debug)]
struct PendingToolCall {
    id: String,
    name: String,
    initial_input: Value,
    raw_arguments: String,
    arguments_delta_seen: bool,
}

struct DeepSeekStreamingAdapter {
    source: StreamEventBox,
    pending: VecDeque<Result<ModelStreamEvent, ModelPortError>>,
    tool_blocks: BTreeMap<u32, PendingToolCall>,
    tool_calls: Vec<ModelToolCall>,
    content: String,
    reasoning: String,
    finish_reason: Option<String>,
    usage: Usage,
    completed: bool,
}

impl DeepSeekStreamingAdapter {
    fn new(source: StreamEventBox) -> Self {
        Self {
            source,
            pending: VecDeque::new(),
            tool_blocks: BTreeMap::new(),
            tool_calls: Vec::new(),
            content: String::new(),
            reasoning: String::new(),
            finish_reason: None,
            usage: Usage::default(),
            completed: false,
        }
    }

    fn finish_tool_block(&mut self, index: u32) {
        let Some(call) = self.tool_blocks.remove(&index) else {
            return;
        };
        let arguments = if call.arguments_delta_seen {
            ToolArguments::parse(call.raw_arguments)
        } else {
            ToolArguments::from_value(call.initial_input)
        };
        self.tool_calls.push(ModelToolCall {
            id: call.id,
            name: call.name,
            arguments,
        });
    }

    fn finish_all_tool_blocks(&mut self) {
        let indices: Vec<u32> = self.tool_blocks.keys().copied().collect();
        for index in indices {
            self.finish_tool_block(index);
        }
    }

    fn accept(&mut self, event: StreamEvent) -> Result<(), ModelPortError> {
        match event {
            StreamEvent::MessageStart { message } => {
                if message.usage != Usage::default() {
                    self.usage = message.usage;
                }
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => match content_block {
                ContentBlockStart::Text { text } if !text.is_empty() => {
                    self.content.push_str(&text);
                    self.pending
                        .push_back(Ok(ModelStreamEvent::ContentDelta { delta: text }));
                }
                ContentBlockStart::Thinking { thinking } if !thinking.is_empty() => {
                    self.reasoning.push_str(&thinking);
                    self.pending
                        .push_back(Ok(ModelStreamEvent::ReasoningDelta { delta: thinking }));
                }
                ContentBlockStart::ToolUse {
                    id, name, input, ..
                } => {
                    self.tool_blocks.insert(
                        index,
                        PendingToolCall {
                            id,
                            name,
                            initial_input: input,
                            raw_arguments: String::new(),
                            arguments_delta_seen: false,
                        },
                    );
                }
                ContentBlockStart::Text { .. }
                | ContentBlockStart::Thinking { .. }
                | ContentBlockStart::ServerToolUse { .. } => {}
            },
            StreamEvent::ContentBlockDelta { index, delta } => match delta {
                Delta::TextDelta { text } => {
                    self.content.push_str(&text);
                    self.pending
                        .push_back(Ok(ModelStreamEvent::ContentDelta { delta: text }));
                }
                Delta::ThinkingDelta { thinking } => {
                    self.reasoning.push_str(&thinking);
                    self.pending
                        .push_back(Ok(ModelStreamEvent::ReasoningDelta { delta: thinking }));
                }
                Delta::InputJsonDelta { partial_json } => {
                    let Some(call) = self.tool_blocks.get_mut(&index) else {
                        return Err(ModelPortError::new(
                            "deepseek_tool_delta_without_start",
                            ModelErrorCategory::Protocol,
                            format!("DeepSeek emitted tool arguments for unopened block {index}"),
                            false,
                        ));
                    };
                    call.arguments_delta_seen = true;
                    call.raw_arguments.push_str(&partial_json);
                }
                Delta::SignatureDelta { .. } => {}
            },
            StreamEvent::ContentBlockStop { index } => self.finish_tool_block(index),
            StreamEvent::MessageDelta { delta, usage } => {
                if let Some(reason) = delta.stop_reason {
                    self.finish_reason = Some(reason);
                }
                if let Some(usage) = usage {
                    self.usage = usage;
                }
            }
            StreamEvent::MessageStop => {
                self.finish_all_tool_blocks();
                let output = ModelOutput {
                    content: self.content.clone(),
                    reasoning_content: (!self.reasoning.is_empty()).then(|| self.reasoning.clone()),
                    tool_calls: std::mem::take(&mut self.tool_calls),
                    finish_reason: finish_reason(self.finish_reason.as_deref())?,
                    usage: runtime_usage(&self.usage),
                };
                self.completed = true;
                self.pending
                    .push_back(Ok(ModelStreamEvent::Completed { output }));
            }
            StreamEvent::Ping => {}
            StreamEvent::Error { error } => {
                return Err(ModelPortError::new(
                    "deepseek_sse_error_event",
                    ModelErrorCategory::Protocol,
                    error.to_string(),
                    false,
                ));
            }
        }
        Ok(())
    }
}

#[async_trait]
impl ModelStream for DeepSeekStreamingAdapter {
    async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some(event);
            }
            if self.completed {
                return None;
            }
            match self.source.next().await {
                Some(Ok(event)) => {
                    if let Err(error) = self.accept(event) {
                        self.completed = true;
                        return Some(Err(error));
                    }
                }
                Some(Err(error)) => {
                    self.completed = true;
                    return Some(Err(model_port_error(error)));
                }
                None => {
                    self.completed = true;
                    return Some(Err(ModelPortError::new(
                        "deepseek_stream_incomplete",
                        ModelErrorCategory::Protocol,
                        "DeepSeek stream ended without MessageStop",
                        true,
                    )));
                }
            }
        }
    }
}

fn model_port_error(error: anyhow::Error) -> ModelPortError {
    if let Some(error) = error.downcast_ref::<LlmError>() {
        return llm_error(error);
    }
    if let Some(error) = error.downcast_ref::<StreamError>() {
        return match error {
            StreamError::Stall { .. } => ModelPortError::new(
                "stream_stall",
                ModelErrorCategory::StreamStall,
                error.to_string(),
                true,
            ),
            StreamError::Overflow { .. } => ModelPortError::new(
                "stream_overflow",
                ModelErrorCategory::Protocol,
                error.to_string(),
                false,
            ),
            StreamError::DurationLimit { .. } => ModelPortError::new(
                "stream_duration_limit",
                ModelErrorCategory::Timeout,
                error.to_string(),
                false,
            ),
        };
    }
    if let Some(error) = error.downcast_ref::<ChatPlanError>() {
        return ModelPortError::new(
            "deepseek_history_replay_invalid",
            ModelErrorCategory::Protocol,
            error.to_string(),
            false,
        );
    }

    // The shared DeepSeek SSE decoder emits this transport-terminal failure
    // after accounting the response as incomplete. It is safe to retry only
    // while Runtime has observed no actionable delta; Runtime owns that gate.
    if error
        .to_string()
        .contains("SSE stream ended before [DONE] or finish_reason")
    {
        return ModelPortError::new(
            "deepseek_stream_incomplete",
            ModelErrorCategory::Protocol,
            error.to_string(),
            true,
        );
    }

    ModelPortError::new(
        "deepseek_unknown",
        ModelErrorCategory::Unknown,
        error.to_string(),
        false,
    )
}

fn llm_error(error: &LlmError) -> ModelPortError {
    let (code, category, retryable) = match error {
        LlmError::RateLimited { .. } => {
            ("deepseek_rate_limited", ModelErrorCategory::RateLimit, true)
        }
        LlmError::ServerError { .. } => ("deepseek_server", ModelErrorCategory::Service, true),
        LlmError::NetworkError(_) => ("deepseek_transport", ModelErrorCategory::Transport, true),
        LlmError::Timeout(_) => ("deepseek_timeout", ModelErrorCategory::Timeout, true),
        LlmError::AuthenticationError(_) => (
            "deepseek_authentication",
            ModelErrorCategory::Authentication,
            false,
        ),
        LlmError::AuthorizationError(_) => (
            "deepseek_authorization",
            ModelErrorCategory::Authentication,
            false,
        ),
        LlmError::InvalidRequest { .. } => (
            "deepseek_invalid_request",
            ModelErrorCategory::Protocol,
            false,
        ),
        LlmError::ModelError(_) => ("deepseek_model", ModelErrorCategory::Service, false),
        LlmError::ContentPolicyError(_) => (
            "deepseek_content_policy",
            ModelErrorCategory::Protocol,
            false,
        ),
        LlmError::ParseError(_) => ("deepseek_parse", ModelErrorCategory::Protocol, false),
        LlmError::ContextLengthError(_) => (
            "deepseek_context_length",
            ModelErrorCategory::Protocol,
            false,
        ),
        LlmError::ApiRequestBudgetExhausted { .. } => (
            "deepseek_request_budget_exhausted",
            ModelErrorCategory::Protocol,
            false,
        ),
        LlmError::ApiRequestBudgetSealed { .. } => (
            "deepseek_request_budget_sealed",
            ModelErrorCategory::Protocol,
            false,
        ),
        LlmError::Other(_) => ("deepseek_other", ModelErrorCategory::Unknown, false),
    };
    ModelPortError::new(code, category, error.to_string(), retryable)
}

fn runtime_accounting(
    requests: ApiRequestBudgetSnapshot,
    actors: ApiRequestActorSnapshot,
    usage: ApiUsageSnapshot,
) -> ModelAccounting {
    let usage_complete = usage.usage_complete();
    let cost_complete = usage.cost_complete();
    let request_complete = requests.in_flight == 0 && requests.started == requests.completed;
    ModelAccounting {
        hard_request_limit: (requests.limit != u32::MAX).then_some(requests.limit),
        root: ActorRequestAccounting {
            started: u64::from(actors.root_started),
            completed: u64::from(actors.root_completed),
            in_flight: u64::from(actors.root_in_flight),
            retries: u64::from(actors.root_retries),
        },
        child: ActorRequestAccounting {
            started: u64::from(actors.child_started),
            completed: u64::from(actors.child_completed),
            in_flight: u64::from(actors.child_in_flight),
            retries: u64::from(actors.child_retries),
        },
        transport_retries: u64::from(requests.retry_attempts),
        runtime_retries: 0,
        sealed_denied: u64::from(requests.sealed_denied),
        exhausted_denied: u64::from(requests.exhausted_denied),
        budget_exhausted: requests.exhausted_denied > 0,
        sealed: requests.sealed,
        complete: request_complete
            && usage_complete
            && cost_complete
            && usage.usage_records_after_seal == 0,
        usage_complete,
        usage_missing: usage.responses_missing_usage > 0,
        usage_incomplete: usage.incomplete_responses > 0,
        billing_unknown: usage.billing_unknown_attempts > 0,
        unpriced: usage.unpriced_usage_responses > 0,
        usage_responses: u64::from(usage.usage_responses),
        usage_missing_responses: u64::from(usage.responses_missing_usage),
        incomplete_responses: u64::from(usage.incomplete_responses),
        billing_unknown_attempts: u64::from(usage.billing_unknown_attempts),
        unpriced_usage_responses: u64::from(usage.unpriced_usage_responses),
        records_after_seal: u64::from(usage.usage_records_after_seal),
        usage: runtime_usage(&usage.usage),
        surface_usage: usage
            .usage_buckets
            .into_iter()
            .map(|bucket| SurfaceUsage {
                surface: match bucket.surface {
                    ApiSurface::StandardChat => RuntimeApiSurface::StandardChat,
                    ApiSurface::StrictChat => RuntimeApiSurface::StrictChat,
                    ApiSurface::Fim => RuntimeApiSurface::Fim,
                },
                model: bucket.model,
                response_count: u64::from(bucket.response_count),
                usage_response_count: u64::from(bucket.usage_responses),
                usage: runtime_usage(&bucket.usage),
                cost_nanousd: to_nano_units(bucket.cost_usd),
                cost_nanocny: to_nano_units(bucket.cost_cny),
            })
            .collect(),
        cost_nanousd: to_nano_units(usage.cost_usd),
        cost_nanocny: to_nano_units(usage.cost_cny),
    }
}

/// Snapshot the one shared DeepSeek request owner into the Runtime's
/// canonical accounting type without sealing further request admission.
#[must_use]
pub(crate) fn model_accounting_snapshot(accounting: &SharedApiRequestBudget) -> ModelAccounting {
    let (requests, actors, usage) = accounting.accounting_snapshot();
    runtime_accounting(requests, actors, usage)
}

fn to_nano_units(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        (value * 1_000_000_000.0).round().min(u64::MAX as f64) as u64
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use futures_util::stream;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::client::deepseek::ResponseMode;
    use crate::config::{ApiProvider, Config, RetryConfig};
    use crate::models::{CacheControl, MessageDelta, SystemBlock};
    use crate::tools::spec::{ToolCapability, ToolSpec};
    use codewhale_runtime::{
        AgentActor, ModelMessage, ReasoningEffort, RunId, ToolInvocationStatus, ToolTransportStatus,
    };

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

    fn runtime_request(streaming: bool) -> ModelRequest {
        ModelRequest {
            run_id: RunId::from("run-1"),
            parent_run_id: None,
            actor: AgentActor::default(),
            model: "deepseek-v4-pro".to_owned(),
            system_prompt: RuntimeSystemPrompt::from_text("稳定系统提示"),
            messages: vec![
                ModelMessage::Assistant {
                    content: None,
                    reasoning_content: Some("原始推理".to_owned()),
                    tool_calls: vec![ModelToolCall {
                        id: "call-1".to_owned(),
                        name: "read_file".to_owned(),
                        arguments: ToolArguments::parse(
                            "{ \"z\" : 1, \"path\" : \"src/lib.rs\", \"a\" : 2 }",
                        ),
                    }],
                },
                ModelMessage::Tool {
                    call_id: "call-1".to_owned(),
                    name: "read_file".to_owned(),
                    content: "ok".to_owned(),
                },
            ],
            tools: vec![ToolDefinition {
                name: "read_file".to_owned(),
                description: "read".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"],
                    "additionalProperties": false
                }),
            }],
            reasoning_effort: ReasoningEffort::Max,
            max_output_tokens: Some(8192),
            streaming,
            request_number: 1,
            attempt: 0,
        }
    }

    async fn streaming_tool_output(arguments_delta: Option<&str>) -> ModelOutput {
        let mut events = vec![Ok(StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlockStart::ToolUse {
                id: "call-1".to_owned(),
                name: "read_file".to_owned(),
                input: json!({}),
                caller: None,
            },
        })];
        if let Some(raw) = arguments_delta {
            events.push(Ok(StreamEvent::ContentBlockDelta {
                index: 0,
                delta: Delta::InputJsonDelta {
                    partial_json: raw.to_owned(),
                },
            }));
        }
        events.extend([
            Ok(StreamEvent::ContentBlockStop { index: 0 }),
            Ok(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: Some("tool_calls".to_owned()),
                    stop_sequence: None,
                },
                usage: Some(Usage::default()),
            }),
            Ok(StreamEvent::MessageStop),
        ]);
        let source: StreamEventBox = Box::pin(stream::iter(events));
        let mut adapter = DeepSeekStreamingAdapter::new(source);
        let ModelStreamEvent::Completed { output } = adapter.next().await.unwrap().unwrap() else {
            panic!("expected completed tool call")
        };
        output
    }

    fn non_streaming_tool_output(raw: &str) -> ModelOutput {
        response_output(MessageResponse {
            id: "message-1".to_owned(),
            r#type: "message".to_owned(),
            role: "assistant".to_owned(),
            content: vec![ContentBlock::ToolUse {
                id: "call-1".to_owned(),
                name: "read_file".to_owned(),
                input: serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned())),
                raw_arguments: Some(raw.to_owned()),
                caller: None,
            }],
            model: "deepseek-v4-pro".to_owned(),
            stop_reason: Some("tool_calls".to_owned()),
            stop_sequence: None,
            container: None,
            usage: Usage::default(),
        })
        .unwrap()
    }

    async fn spawn_truncated_chat_server(chunked: bool) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind truncated response fixture");
        let address = listener.local_addr().expect("fixture address");
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept chat request");
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).await;
            let response: &[u8] = if chunked {
                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n80\r\n{\"id\":\"truncated\""
            } else {
                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 512\r\nconnection: close\r\n\r\n{\"id\":\"truncated\""
            };
            socket
                .write_all(response)
                .await
                .expect("write truncated body");
            socket.shutdown().await.expect("close truncated response");
        });
        (format!("http://{address}"), task)
    }

    #[test]
    fn canonical_request_plan_preserves_reasoning_raw_arguments_and_stable_body() {
        let mut request = runtime_request(true);
        request.system_prompt.blocks.push(RuntimeSystemPromptBlock {
            text: "动态尾部".to_owned(),
            cache_control: PromptCacheControl::Volatile,
        });
        let plan = crate::client::deepseek::plan_runtime_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            false,
            &request,
        )
        .unwrap()
        .unwrap();
        assert_eq!(plan.response_mode, ResponseMode::Streaming);
        assert_eq!(plan.surface, ApiSurface::StandardChat);
        assert_eq!(plan.url, "https://api.deepseek.com/chat/completions");
        assert_eq!(plan.body["stream"], true);
        assert_eq!(plan.body["reasoning_effort"], "max");
        assert_eq!(
            plan.body["messages"][0]["content"],
            "稳定系统提示\n\n---\n\n动态尾部"
        );
        assert_eq!(plan.body["tools"][0]["function"]["name"], "read_file");
        let assistant = plan.body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "assistant")
            .unwrap();
        assert_eq!(assistant["reasoning_content"], json!("原始推理"));
        assert_eq!(
            assistant["tool_calls"][0]["function"]["arguments"],
            json!("{ \"z\" : 1, \"path\" : \"src/lib.rs\", \"a\" : 2 }")
        );
        let repeated = crate::client::deepseek::plan_runtime_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            false,
            &request,
        )
        .unwrap()
        .unwrap();
        assert_eq!(plan, repeated);
    }

    #[test]
    fn canonical_request_plan_rejects_empty_reasoning_before_transport() {
        let mut request = runtime_request(true);
        let ModelMessage::Assistant {
            reasoning_content, ..
        } = &mut request.messages[0]
        else {
            unreachable!()
        };
        *reasoning_content = Some(String::new());

        let error = crate::client::deepseek::plan_runtime_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            false,
            &request,
        )
        .expect_err("empty reasoning provenance must fail before sender");
        assert_eq!(
            error,
            ChatPlanError::MissingReasoningContent { message_index: 0 }
        );
    }

    #[test]
    fn canonical_prompt_preserves_block_order_and_cache_semantics() {
        let prompt = SystemPrompt::Blocks(vec![
            SystemBlock {
                block_type: "text".to_owned(),
                text: "稳定前缀".to_owned(),
                cache_control: None,
            },
            SystemBlock {
                block_type: "text".to_owned(),
                text: "显式稳定".to_owned(),
                cache_control: Some(CacheControl {
                    cache_type: "ephemeral".to_owned(),
                }),
            },
            SystemBlock {
                block_type: "text".to_owned(),
                text: "每轮变化".to_owned(),
                cache_control: None,
            },
        ]);

        let canonical = canonical_system_prompt(&prompt);
        assert_eq!(canonical.blocks[0].text, "稳定前缀");
        assert_eq!(canonical.blocks[1].text, "显式稳定");
        assert_eq!(canonical.blocks[2].text, "每轮变化");
        assert_eq!(
            canonical.blocks[0].cache_control,
            PromptCacheControl::Stable
        );
        assert_eq!(
            canonical.blocks[1].cache_control,
            PromptCacheControl::Stable
        );
        assert_eq!(
            canonical.blocks[2].cache_control,
            PromptCacheControl::Volatile
        );
    }

    #[test]
    fn request_projection_replays_empty_and_malformed_tool_arguments_verbatim() {
        for raw in ["", "{not-json"] {
            let mut request = runtime_request(false);
            let ModelMessage::Assistant { tool_calls, .. } = &mut request.messages[0] else {
                unreachable!()
            };
            tool_calls[0].arguments = ToolArguments::parse(raw);
            let plan = crate::client::deepseek::plan_runtime_chat(
                ApiProvider::Deepseek,
                "https://api.deepseek.com",
                None,
                false,
                &request,
            )
            .unwrap()
            .unwrap();
            let assistant = plan.body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["role"] == "assistant")
                .unwrap();
            assert_eq!(
                assistant["tool_calls"][0]["function"]["arguments"],
                json!(raw)
            );
        }
    }

    #[test]
    fn strict_planning_is_beta_only_and_falls_back_without_dropping_tools() {
        let request = runtime_request(true);
        let strict = crate::client::deepseek::plan_runtime_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            true,
            &request,
        )
        .unwrap()
        .unwrap();
        assert_eq!(strict.surface, ApiSurface::StrictChat);
        assert_eq!(strict.url, "https://api.deepseek.com/beta/chat/completions");

        let mut incompatible = request;
        incompatible.tools.push(ToolDefinition {
            name: "optional_tool".to_owned(),
            description: "incompatible strict fixture".to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {"optional": {"type": "string"}},
                "required": [],
                "additionalProperties": false
            }),
        });
        let standard = crate::client::deepseek::plan_runtime_chat(
            ApiProvider::Deepseek,
            "https://api.deepseek.com",
            None,
            true,
            &incompatible,
        )
        .unwrap()
        .unwrap();
        assert_eq!(standard.surface, ApiSurface::StandardChat);
        assert_eq!(standard.url, "https://api.deepseek.com/chat/completions");
        assert_eq!(standard.body["tools"].as_array().unwrap().len(), 2);
        assert!(
            standard.body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .all(|tool| tool["function"].get("strict").is_none())
        );
    }

    #[test]
    fn non_streaming_response_keeps_provider_tool_arguments_verbatim() {
        let raw = "{ \"z\" : 1, \"path\" : \"src/lib.rs\", \"a\" : 2 }";
        let output = response_output(MessageResponse {
            id: "message-1".to_owned(),
            r#type: "message".to_owned(),
            role: "assistant".to_owned(),
            content: vec![ContentBlock::ToolUse {
                id: "call-1".to_owned(),
                name: "read_file".to_owned(),
                input: serde_json::from_str(raw).unwrap(),
                raw_arguments: Some(raw.to_owned()),
                caller: None,
            }],
            model: "deepseek-v4-pro".to_owned(),
            stop_reason: Some("tool_calls".to_owned()),
            stop_sequence: None,
            container: None,
            usage: Usage::default(),
        })
        .unwrap();
        assert_eq!(output.tool_calls[0].arguments.raw, raw);
        assert_eq!(
            output.tool_calls[0].arguments.parsed,
            Some(json!({"z": 1, "path": "src/lib.rs", "a": 2}))
        );
    }

    #[tokio::test]
    async fn streaming_and_non_streaming_tool_arguments_preserve_the_same_raw_truth() {
        for raw in ["", "{not-json", "{ \"path\" : \"src/lib.rs\" }"] {
            let streamed = streaming_tool_output(Some(raw)).await;
            let non_streamed = non_streaming_tool_output(raw);
            assert_eq!(streamed.tool_calls[0].arguments.raw, raw);
            assert_eq!(
                streamed.tool_calls[0].arguments,
                non_streamed.tool_calls[0].arguments
            );
        }

        let absent_delta = streaming_tool_output(None).await;
        assert_eq!(absent_delta.tool_calls[0].arguments.raw, "{}");
        assert_eq!(absent_delta.tool_calls[0].arguments.parsed, Some(json!({})));

        let explicit_empty = streaming_tool_output(Some("")).await;
        assert_eq!(explicit_empty.tool_calls[0].arguments.raw, "");
        assert!(explicit_empty.tool_calls[0].arguments.parsed.is_none());
    }

    #[tokio::test]
    async fn stream_preserves_raw_malformed_arguments_and_full_output() {
        let events = vec![
            Ok(StreamEvent::MessageStart {
                message: MessageResponse {
                    id: "m1".to_owned(),
                    r#type: "message".to_owned(),
                    role: "assistant".to_owned(),
                    content: Vec::new(),
                    model: "deepseek-v4-pro".to_owned(),
                    stop_reason: None,
                    stop_sequence: None,
                    container: None,
                    usage: Usage::default(),
                },
            }),
            Ok(StreamEvent::ContentBlockDelta {
                index: 0,
                delta: Delta::ThinkingDelta {
                    thinking: "先检查".to_owned(),
                },
            }),
            Ok(StreamEvent::ContentBlockStart {
                index: 1,
                content_block: ContentBlockStart::ToolUse {
                    id: "call-bad".to_owned(),
                    name: "read_file".to_owned(),
                    input: Value::Null,
                    caller: None,
                },
            }),
            Ok(StreamEvent::ContentBlockDelta {
                index: 1,
                delta: Delta::InputJsonDelta {
                    partial_json: "{not-json".to_owned(),
                },
            }),
            Ok(StreamEvent::ContentBlockStop { index: 1 }),
            Ok(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: Some("tool_calls".to_owned()),
                    stop_sequence: None,
                },
                usage: Some(Usage {
                    input_tokens: 10,
                    output_tokens: 4,
                    ..Usage::default()
                }),
            }),
            Ok(StreamEvent::MessageStop),
        ];
        let source: StreamEventBox = Box::pin(stream::iter(events));
        let mut adapter = DeepSeekStreamingAdapter::new(source);
        assert!(matches!(
            adapter.next().await.unwrap().unwrap(),
            ModelStreamEvent::ReasoningDelta { delta } if delta == "先检查"
        ));
        let ModelStreamEvent::Completed { output } = adapter.next().await.unwrap().unwrap() else {
            panic!("expected completion");
        };
        assert_eq!(output.reasoning_content.as_deref(), Some("先检查"));
        assert_eq!(output.tool_calls[0].arguments.raw, "{not-json");
        assert!(output.tool_calls[0].arguments.parsed.is_none());
        assert_eq!(output.usage.input_tokens, 10);
    }

    #[tokio::test]
    async fn stream_eof_before_message_stop_is_retryable() {
        let source: StreamEventBox = Box::pin(stream::empty());
        let mut adapter = DeepSeekStreamingAdapter::new(source);
        let error = adapter.next().await.unwrap().unwrap_err();

        assert_eq!(error.code, "deepseek_stream_incomplete");
        assert_eq!(error.category, ModelErrorCategory::Protocol);
        assert!(error.retryable);
    }

    #[test]
    fn deepseek_decoder_incomplete_terminal_is_retryable() {
        let error = model_port_error(anyhow::anyhow!(
            "DeepSeek SSE stream ended before [DONE] or finish_reason; partial output was not accepted"
        ));

        assert_eq!(error.code, "deepseek_stream_incomplete");
        assert_eq!(error.category, ModelErrorCategory::Protocol);
        assert!(error.retryable);
    }

    #[test]
    fn typed_stream_transport_is_retryable_by_runtime() {
        let error = model_port_error(anyhow::Error::new(LlmError::NetworkError(
            "Stream read error: connection reset while reading response body".to_owned(),
        )));

        assert_eq!(error.code, "deepseek_transport");
        assert_eq!(error.category, ModelErrorCategory::Transport);
        assert!(error.retryable);
    }

    #[test]
    fn typed_non_streaming_header_timeout_is_retryable_by_runtime() {
        let error = model_port_error(anyhow::Error::new(LlmError::Timeout(
            std::time::Duration::from_secs(5),
        )));

        assert_eq!(error.code, "deepseek_timeout");
        assert_eq!(error.category, ModelErrorCategory::Timeout);
        assert!(error.retryable);
    }

    #[tokio::test]
    async fn truncated_non_streaming_bodies_map_to_retryable_transport_errors() {
        for chunked in [false, true] {
            let (base_url, server) = spawn_truncated_chat_server(chunked).await;
            let client = DeepSeekClient::new(&Config {
                provider: Some("deepseek".to_owned()),
                api_key: Some("test-key-not-sent".to_owned()),
                base_url: Some(base_url.clone()),
                retry: Some(RetryConfig {
                    enabled: Some(false),
                    max_retries: Some(0),
                    initial_delay: Some(0.0),
                    max_delay: Some(0.0),
                    exponential_base: Some(1.0),
                }),
                ..Config::default()
            })
            .expect("local DeepSeek client");
            let mut plan = crate::client::deepseek::plan_runtime_chat(
                ApiProvider::Deepseek,
                "https://api.deepseek.com",
                None,
                false,
                &runtime_request(false),
            )
            .unwrap()
            .unwrap();
            plan.url = format!("{base_url}/chat/completions");

            let error = match client.create_planned_deepseek_message(plan).await {
                Ok(_) => panic!("truncated 200 response must not be accepted"),
                Err(error) => model_port_error(error),
            };
            server.await.expect("truncated response fixture");

            assert_eq!(error.code, "deepseek_transport");
            assert_eq!(error.category, ModelErrorCategory::Transport);
            assert!(error.retryable);
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

    #[test]
    fn accounting_projection_keeps_every_ledger_counter_and_nano_cost() {
        use crate::client::request_budget::ApiUsageBucket;

        let accounting = runtime_accounting(
            ApiRequestBudgetSnapshot {
                limit: 12,
                started: 5,
                in_flight: 0,
                completed: 5,
                retry_attempts: 2,
                exhausted_denied: 1,
                sealed_denied: 3,
                sealed: true,
            },
            ApiRequestActorSnapshot {
                root_started: 3,
                root_in_flight: 0,
                root_completed: 3,
                root_retries: 1,
                child_started: 2,
                child_in_flight: 0,
                child_completed: 2,
                child_retries: 1,
            },
            ApiUsageSnapshot {
                usage: Usage {
                    input_tokens: 101,
                    output_tokens: 29,
                    prompt_cache_hit_tokens: Some(80),
                    prompt_cache_miss_tokens: Some(21),
                    prompt_cache_write_tokens: Some(4),
                    reasoning_tokens: Some(9),
                    reasoning_replay_tokens: Some(7),
                    server_tool_use: None,
                },
                usage_responses: 4,
                standard_chat_responses: 3,
                strict_chat_responses: 1,
                fim_responses: 0,
                usage_buckets: vec![ApiUsageBucket {
                    model: "deepseek-v4-pro".to_owned(),
                    surface: ApiSurface::StrictChat,
                    response_count: 1,
                    usage_responses: 1,
                    usage: Usage {
                        input_tokens: 31,
                        output_tokens: 11,
                        ..Usage::default()
                    },
                    cost_usd: 0.000_000_123_4,
                    cost_cny: 0.000_000_987_6,
                }],
                responses_missing_usage: 1,
                incomplete_responses: 2,
                billing_unknown_attempts: 3,
                unpriced_usage_responses: 4,
                usage_records_after_seal: 5,
                cost_usd: 0.000_000_123_4,
                cost_cny: 0.000_000_987_6,
            },
        );

        assert_eq!(accounting.hard_request_limit, Some(12));
        assert_eq!(accounting.root.started, 3);
        assert_eq!(accounting.root.retries, 1);
        assert_eq!(accounting.child.started, 2);
        assert_eq!(accounting.child.retries, 1);
        assert_eq!(accounting.transport_retries, 2);
        assert_eq!(accounting.exhausted_denied, 1);
        assert_eq!(accounting.sealed_denied, 3);
        assert!(accounting.budget_exhausted);
        assert!(!accounting.complete);
        assert_eq!(accounting.usage_responses, 4);
        assert_eq!(accounting.usage_missing_responses, 1);
        assert_eq!(accounting.incomplete_responses, 2);
        assert_eq!(accounting.billing_unknown_attempts, 3);
        assert_eq!(accounting.unpriced_usage_responses, 4);
        assert_eq!(accounting.records_after_seal, 5);
        assert_eq!(accounting.usage.cache_hit_tokens, 80);
        assert_eq!(accounting.cost_nanousd, 123);
        assert_eq!(accounting.cost_nanocny, 988);
        assert_eq!(accounting.surface_usage.len(), 1);
        assert_eq!(accounting.surface_usage[0].response_count, 1);
        assert_eq!(accounting.surface_usage[0].usage_response_count, 1);
        assert_eq!(accounting.surface_usage[0].cost_nanousd, 123);
        assert_eq!(accounting.surface_usage[0].cost_nanocny, 988);
    }
}
