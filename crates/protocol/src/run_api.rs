//! Transport-neutral commands and projections for the canonical Run API.
//!
//! These DTOs deliberately do not expose [`RunRequest`](crate::agent_runtime::RunRequest).
//! Host-owned prompt, actor, provider, execution-fingerprint, and accounting
//! fields are composed by the application service rather than accepted from a
//! transport client.

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::agent_runtime::{
    InteractionId, ModelAccounting, ReasoningEffort, RunId, RunLimits, RunPurpose,
    StoredRuntimeEvent, TerminalState, ToolPolicy, Usage, UserInteractionResponse,
};

/// Current schema version for Run API command and response envelopes.
pub const RUN_API_SCHEMA_VERSION: u32 = 4;
pub const DEFAULT_RUN_LIST_LIMIT: u32 = 50;
pub const MAX_RUN_LIST_LIMIT: u32 = 200;

/// Explicit product controls accepted when starting a root run.
///
/// These controls affect local execution posture. Immutable recovery facts
/// such as provider, tool-catalog hash, and execution fingerprint remain
/// application-owned and are not part of the public command.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunProductControls {
    #[serde(default)]
    pub auto_approve: bool,
    #[serde(default)]
    pub trust_mode: bool,
    #[serde(default)]
    pub allow_sandbox_elevation: bool,
    /// Whether the caller can resolve durable approval and user-input events.
    #[serde(default)]
    pub interactive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
}

/// User-controlled input for a new root run.
///
/// The application service expands this value into the canonical internal
/// `RunRequest`; transports cannot supply host-owned runtime fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct StartRunCommand {
    pub input: String,
    pub workspace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: ReasoningEffort,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Maximum physical DeepSeek HTTP requests started by route selection,
    /// transport retries, the root run, and all of its child runs. The hard
    /// limit becomes durable through canonical model accounting at RunCreated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_api_requests: Option<NonZeroU32>,
    /// Whether the model response uses the streaming DeepSeek surface. Plain
    /// one-shot CLI execution is non-streaming; Agent/SSE execution streams.
    pub streaming: bool,
    #[serde(default)]
    pub tool_policy: ToolPolicy,
    #[serde(default)]
    pub limits: RunLimits,
    #[serde(default)]
    pub controls: RunProductControls,
}

/// User input for a new root run that continues one terminal root run.
///
/// Model, prompt, tools, execution posture, and limits are inherited from the
/// source run and revalidated by the application. A continuation is never a
/// resume and never a child Agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ContinueRunCommand {
    pub run_id: RunId,
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_workspace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompactRunCommand {
    pub run_id: RunId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_workspace: Option<String>,
}

/// Durable identity of a creation command whose reserved run has not reached
/// `RunCreated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingCreationKind {
    Start,
    Continue,
    Compact,
}

/// Read-only projection of one pending creation intent.
///
/// The original command payload remains private to the RunStore. This
/// projection contains enough identity for a client to request recovery
/// without resubmitting or reconstructing that payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PendingCreationSummary {
    pub creation_request_id: String,
    pub reserved_run_id: RunId,
    pub kind: PendingCreationKind,
    pub workspace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<RunId>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unknown_billing: bool,
    pub created_at_unix_ms: u64,
}

/// One versioned application command submitted over HTTP or stdio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunCommandEnvelope {
    pub schema_version: u32,
    pub request_id: String,
    pub command: RunCommand,
}

/// Minimal command set for the canonical local Run API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunCommand {
    Start(StartRunCommand),
    Continue(ContinueRunCommand),
    Compact(CompactRunCommand),
    ListRoots {
        workspace: String,
        #[serde(default = "default_run_list_limit")]
        limit: u32,
    },
    ListPendingCreations {
        workspace: String,
        #[serde(default = "default_run_list_limit")]
        limit: u32,
    },
    RecoverCreation {
        creation_request_id: String,
    },
    Get {
        run_id: RunId,
    },
    Events {
        run_id: RunId,
        #[serde(default)]
        after_sequence: u64,
    },
    Resume {
        run_id: RunId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_workspace: Option<String>,
    },
    Steer {
        run_id: RunId,
        content: String,
    },
    Interrupt {
        run_id: RunId,
    },
    Cancel {
        run_id: RunId,
    },
    ResolveInteraction {
        run_id: RunId,
        interaction_id: InteractionId,
        response: UserInteractionResponse,
    },
}

/// Read-only projection of one canonical run.
///
/// Every field is derived from the existing Agent runtime request/snapshot;
/// this type does not introduce another lifecycle or terminal status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunView {
    pub run_id: RunId,
    pub purpose: RunPurpose,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continued_from_run_id: Option<RunId>,
    pub model: String,
    pub workspace: String,
    pub last_sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalState>,
    pub usage: Usage,
    pub accounting: ModelAccounting,
    pub runtime_model_requests: u32,
    pub runtime_retries: u32,
    pub tool_calls: u32,
    pub local_turns: u32,
}

/// Lightweight RunStore projection used for workspace-scoped session lookup.
///
/// Entries are root runs, including internal context-compaction roots.
/// Continuation lineage is represented by `continued_from_run_id`; child
/// hierarchy remains represented exclusively by `parent_run_id` on full run
/// views and events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RootRunSummary {
    pub run_id: RunId,
    pub purpose: RunPurpose,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continued_from_run_id: Option<RunId>,
    pub workspace: String,
    pub last_sequence: u64,
    pub terminal: bool,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

/// Stable machine-readable error codes shared by HTTP and stdio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunApiErrorCode {
    InvalidRequest,
    RunAlreadyExists,
    RunNotFound,
    RunAlreadyRunning,
    RunNotActive,
    RunRecoveryRequired,
    RunTerminal,
    RunContinuationInvalid,
    RunEnvironmentMismatch,
    EventCursorAhead,
    RunStoreFailed,
    InteractionNotPending,
    InteractionMismatch,
    InteractionAlreadyResolved,
    InvalidInteractionResponse,
}

/// Typed Run API failure. Human-readable text is supplementary to `code`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunApiError {
    pub code: RunApiErrorCode,
    pub message: Box<str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation: Option<Box<CreationRecoveryContext>>,
}

/// Typed recovery facts present only on creation-delivery errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CreationRecoveryContext {
    pub creation_request_id: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unknown_billing: bool,
}

/// Transport-neutral result payload for every Run command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunCommandResult {
    Run {
        run: Box<RunView>,
    },
    Events {
        run_id: RunId,
        after_sequence: u64,
        events: Vec<StoredRuntimeEvent>,
    },
    Runs {
        workspace: String,
        runs: Vec<RootRunSummary>,
    },
    PendingCreations {
        workspace: String,
        creations: Vec<PendingCreationSummary>,
    },
    Accepted {
        run_id: RunId,
        last_sequence: u64,
    },
    Error {
        error: RunApiError,
    },
}

const fn default_run_list_limit() -> u32 {
    DEFAULT_RUN_LIST_LIMIT
}

const fn is_false(value: &bool) -> bool {
    !*value
}

/// Versioned response correlated with one [`RunCommandEnvelope`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunCommandResponse {
    pub schema_version: u32,
    pub request_id: String,
    pub result: RunCommandResult,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::agent_runtime::{
        AGENT_RUNTIME_EVENT_SCHEMA_VERSION, CommandId, RuntimeEventId, RuntimeEventKind,
        StoredRuntimeEvent, ToolPolicy,
    };

    fn start_command() -> StartRunCommand {
        StartRunCommand {
            input: "修复边界错误".to_owned(),
            workspace: "/workspace/project".to_owned(),
            model: Some("deepseek-v4-flash".to_owned()),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: Some(8_192),
            max_api_requests: NonZeroU32::new(9),
            streaming: true,
            tool_policy: ToolPolicy {
                enabled: true,
                allowed: Some(vec!["read_file".to_owned(), "apply_patch".to_owned()]),
                denied: Vec::new(),
            },
            limits: RunLimits {
                max_turns: 12,
                max_model_requests: 10,
                max_model_retries: 1,
                max_tool_calls: 32,
                max_depth: 2,
                max_concurrent_children: 4,
                model_event_idle_ms: Some(30_000),
                wall_time_ms: Some(600_000),
            },
            controls: RunProductControls {
                auto_approve: true,
                trust_mode: false,
                allow_sandbox_elevation: false,
                interactive: true,
                sandbox: Some("workspace_write".to_owned()),
            },
        }
    }

    fn run_view() -> RunView {
        RunView {
            run_id: RunId::from("run-1"),
            purpose: RunPurpose::Agent,
            parent_run_id: None,
            continued_from_run_id: None,
            model: "deepseek-v4-flash".to_owned(),
            workspace: "/workspace/project".to_owned(),
            last_sequence: 2,
            terminal: None,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 2,
                cache_hit_tokens: 8,
                cache_miss_tokens: 2,
                cache_write_tokens: 0,
                reasoning_tokens: 1,
                reasoning_replay_tokens: 0,
            },
            accounting: ModelAccounting::default(),
            runtime_model_requests: 1,
            runtime_retries: 0,
            tool_calls: 0,
            local_turns: 1,
        }
    }

    #[test]
    fn start_command_json_is_stable_and_excludes_host_owned_fields() {
        let envelope = RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: "request-1".to_owned(),
            command: RunCommand::Start(start_command()),
        };
        let encoded = serde_json::to_value(&envelope).expect("serialize start command");
        assert_eq!(
            encoded,
            json!({
                "schema_version": 4,
                "request_id": "request-1",
                "command": {
                    "kind": "start",
                    "input": "修复边界错误",
                    "workspace": "/workspace/project",
                    "model": "deepseek-v4-flash",
                    "reasoning_effort": "high",
                    "max_output_tokens": 8192,
                    "max_api_requests": 9,
                    "streaming": true,
                    "tool_policy": {
                        "enabled": true,
                        "allowed": ["read_file", "apply_patch"],
                        "denied": []
                    },
                    "limits": {
                        "max_turns": 12,
                        "max_model_requests": 10,
                        "max_model_retries": 1,
                        "max_tool_calls": 32,
                        "max_depth": 2,
                        "max_concurrent_children": 4,
                        "model_event_idle_ms": 30000,
                        "wall_time_ms": 600000
                    },
                    "controls": {
                        "auto_approve": true,
                        "trust_mode": false,
                        "allow_sandbox_elevation": false,
                        "interactive": true,
                        "sandbox": "workspace_write"
                    }
                }
            })
        );
        let command = encoded
            .get("command")
            .and_then(Value::as_object)
            .expect("command object");
        for forbidden in [
            "system_prompt",
            "actor",
            "provider",
            "api_key",
            "credential",
            "authorization",
            "tool_catalog_sha256",
            "execution_fingerprint_sha256",
            "accounting_baseline",
            "transcript",
            "parent_run_id",
            "continued_from_run_id",
        ] {
            assert!(
                !command.contains_key(forbidden),
                "start command exposed host-owned field {forbidden}"
            );
        }
        let decoded: RunCommandEnvelope =
            serde_json::from_value(encoded).expect("round-trip start command");
        assert_eq!(decoded, envelope);
    }

    #[test]
    fn all_command_tags_round_trip() {
        let commands = vec![
            RunCommand::Start(start_command()),
            RunCommand::Continue(ContinueRunCommand {
                run_id: RunId::from("run-1"),
                input: "继续修复".to_owned(),
                expected_workspace: Some("/workspace/project".to_owned()),
            }),
            RunCommand::Compact(CompactRunCommand {
                run_id: RunId::from("run-1"),
                expected_workspace: Some("/workspace/project".to_owned()),
            }),
            RunCommand::ListRoots {
                workspace: "/workspace/project".to_owned(),
                limit: 25,
            },
            RunCommand::ListPendingCreations {
                workspace: "/workspace/project".to_owned(),
                limit: 25,
            },
            RunCommand::RecoverCreation {
                creation_request_id: "creation-1".to_owned(),
            },
            RunCommand::Get {
                run_id: RunId::from("run-1"),
            },
            RunCommand::Events {
                run_id: RunId::from("run-1"),
                after_sequence: 7,
            },
            RunCommand::Resume {
                run_id: RunId::from("run-1"),
                expected_workspace: None,
            },
            RunCommand::Steer {
                run_id: RunId::from("run-1"),
                content: "先修测试".to_owned(),
            },
            RunCommand::Interrupt {
                run_id: RunId::from("run-1"),
            },
            RunCommand::Cancel {
                run_id: RunId::from("run-1"),
            },
            RunCommand::ResolveInteraction {
                run_id: RunId::from("run-1"),
                interaction_id: InteractionId::from("interaction-1"),
                response: UserInteractionResponse::Approved,
            },
        ];
        let expected = [
            "start",
            "continue",
            "compact",
            "list_roots",
            "list_pending_creations",
            "recover_creation",
            "get",
            "events",
            "resume",
            "steer",
            "interrupt",
            "cancel",
            "resolve_interaction",
        ];

        for (command, expected_kind) in commands.into_iter().zip(expected) {
            let encoded = serde_json::to_value(&command).expect("serialize command");
            assert_eq!(encoded["kind"], expected_kind);
            assert_eq!(
                serde_json::from_value::<RunCommand>(encoded).expect("deserialize command"),
                command
            );
        }
    }

    #[test]
    fn continue_command_exposes_only_source_input_and_workspace_guard() {
        let command = RunCommand::Continue(ContinueRunCommand {
            run_id: RunId::from("source-1"),
            input: "继续完成测试".to_owned(),
            expected_workspace: Some("/workspace/project".to_owned()),
        });
        assert_eq!(
            serde_json::to_value(command).expect("serialize continue"),
            json!({
                "kind": "continue",
                "run_id": "source-1",
                "input": "继续完成测试",
                "expected_workspace": "/workspace/project"
            })
        );
    }

    #[test]
    fn root_list_limit_defaults_to_product_value() {
        let command: RunCommand = serde_json::from_value(json!({
            "kind": "list_roots",
            "workspace": "/workspace/project"
        }))
        .expect("deserialize root list");
        assert_eq!(
            command,
            RunCommand::ListRoots {
                workspace: "/workspace/project".to_owned(),
                limit: DEFAULT_RUN_LIST_LIMIT,
            }
        );
    }

    #[test]
    fn resume_expected_workspace_is_optional_and_exact() {
        let without_workspace = RunCommand::Resume {
            run_id: RunId::from("run-1"),
            expected_workspace: None,
        };
        let encoded = serde_json::to_value(&without_workspace).expect("serialize resume");
        assert_eq!(encoded, json!({ "kind": "resume", "run_id": "run-1" }));
        assert_eq!(
            serde_json::from_value::<RunCommand>(encoded).expect("default missing workspace"),
            without_workspace
        );

        let exact = RunCommand::Resume {
            run_id: RunId::from("run-1"),
            expected_workspace: Some("/workspace/项目".to_owned()),
        };
        assert_eq!(
            serde_json::to_value(&exact).expect("serialize expected workspace")["expected_workspace"],
            "/workspace/项目"
        );
    }

    #[test]
    fn response_uses_existing_run_view_and_stored_event_envelope() {
        let event = StoredRuntimeEvent {
            schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: RunId::from("run-1"),
            parent_run_id: None,
            event_id: RuntimeEventId("steer-1".to_owned()),
            sequence: 2,
            occurred_at_unix_ms: 123,
            event: RuntimeEventKind::SteerQueued {
                command_id: CommandId::from("steer-command-1"),
                content: "先修测试".to_owned(),
            },
        };
        let response = RunCommandResponse {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: "request-events".to_owned(),
            result: RunCommandResult::Events {
                run_id: RunId::from("run-1"),
                after_sequence: 1,
                events: vec![event.clone()],
            },
        };
        let encoded = serde_json::to_value(&response).expect("serialize response");
        assert_eq!(encoded["result"]["kind"], "events");
        assert_eq!(encoded["result"]["events"][0], json!(event));
        assert_eq!(
            serde_json::from_value::<RunCommandResponse>(encoded).expect("round-trip response"),
            response
        );

        let run_response = RunCommandResponse {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: "request-get".to_owned(),
            result: RunCommandResult::Run {
                run: Box::new(run_view()),
            },
        };
        assert_eq!(
            serde_json::from_str::<RunCommandResponse>(
                &serde_json::to_string(&run_response).expect("serialize run response")
            )
            .expect("deserialize run response"),
            run_response
        );
    }

    #[test]
    fn pending_creation_projection_and_unknown_billing_error_are_typed() {
        let response = RunCommandResponse {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: "list-pending".to_owned(),
            result: RunCommandResult::PendingCreations {
                workspace: "/workspace/project".to_owned(),
                creations: vec![PendingCreationSummary {
                    creation_request_id: "creation-auto".to_owned(),
                    reserved_run_id: RunId::from("reserved-auto"),
                    kind: PendingCreationKind::Start,
                    workspace: "/workspace/project".to_owned(),
                    source_run_id: None,
                    unknown_billing: true,
                    created_at_unix_ms: 123,
                }],
            },
        };
        let encoded = serde_json::to_value(&response).expect("serialize pending response");
        assert_eq!(encoded["result"]["kind"], "pending_creations");
        assert_eq!(
            encoded["result"]["creations"][0]["creation_request_id"],
            "creation-auto"
        );
        assert_eq!(
            encoded["result"]["creations"][0]["reserved_run_id"],
            "reserved-auto"
        );
        assert_eq!(encoded["result"]["creations"][0]["unknown_billing"], true);
        assert_eq!(
            serde_json::from_value::<RunCommandResponse>(encoded).expect("round-trip pending"),
            response
        );

        let error = RunApiError {
            code: RunApiErrorCode::RunRecoveryRequired,
            message: "不会重复路由".into(),
            run_id: Some(RunId::from("reserved-auto")),
            terminal: None,
            creation: Some(Box::new(CreationRecoveryContext {
                creation_request_id: "creation-auto".to_owned(),
                unknown_billing: true,
            })),
        };
        let encoded = serde_json::to_value(&error).expect("serialize recovery error");
        assert_eq!(encoded["creation"]["creation_request_id"], "creation-auto");
        assert_eq!(encoded["run_id"], "reserved-auto");
        assert_eq!(encoded["creation"]["unknown_billing"], true);
    }

    #[test]
    fn error_codes_have_stable_snake_case_values() {
        let cases = [
            (RunApiErrorCode::InvalidRequest, "invalid_request"),
            (RunApiErrorCode::RunAlreadyExists, "run_already_exists"),
            (RunApiErrorCode::RunNotFound, "run_not_found"),
            (RunApiErrorCode::RunAlreadyRunning, "run_already_running"),
            (RunApiErrorCode::RunNotActive, "run_not_active"),
            (
                RunApiErrorCode::RunRecoveryRequired,
                "run_recovery_required",
            ),
            (RunApiErrorCode::RunTerminal, "run_terminal"),
            (
                RunApiErrorCode::RunEnvironmentMismatch,
                "run_environment_mismatch",
            ),
            (RunApiErrorCode::EventCursorAhead, "event_cursor_ahead"),
            (RunApiErrorCode::RunStoreFailed, "run_store_failed"),
        ];

        for (code, expected) in cases {
            assert_eq!(serde_json::to_value(code).unwrap(), expected);
            assert_eq!(
                serde_json::from_value::<RunApiErrorCode>(json!(expected)).unwrap(),
                code
            );
        }
    }

    #[test]
    fn unknown_fields_are_rejected_and_schema_version_remains_application_visible() {
        let mut encoded = serde_json::to_value(RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: "request-1".to_owned(),
            command: RunCommand::Start(start_command()),
        })
        .unwrap();
        encoded
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_owned(), json!(true));
        assert!(serde_json::from_value::<RunCommandEnvelope>(encoded).is_err());

        let mut command = serde_json::to_value(RunCommand::Start(start_command())).unwrap();
        command
            .as_object_mut()
            .unwrap()
            .insert("system_prompt".to_owned(), json!("transport injection"));
        assert!(serde_json::from_value::<RunCommand>(command).is_err());

        let mut missing_streaming =
            serde_json::to_value(RunCommand::Start(start_command())).unwrap();
        missing_streaming
            .as_object_mut()
            .unwrap()
            .remove("streaming");
        assert!(
            serde_json::from_value::<RunCommand>(missing_streaming).is_err(),
            "streaming must be explicit so plain exec and Agent runs cannot silently diverge"
        );

        let mut zero_physical_budget =
            serde_json::to_value(RunCommand::Start(start_command())).unwrap();
        zero_physical_budget["max_api_requests"] = json!(0);
        assert!(
            serde_json::from_value::<RunCommand>(zero_physical_budget).is_err(),
            "a physical HTTP budget of zero is not a runnable request"
        );

        let future = json!({
            "schema_version": 99,
            "request_id": "future-request",
            "command": { "kind": "get", "run_id": "run-1" }
        });
        let decoded: RunCommandEnvelope = serde_json::from_value(future).unwrap();
        assert_eq!(decoded.schema_version, 99);
        assert_ne!(decoded.schema_version, RUN_API_SCHEMA_VERSION);
    }
}
