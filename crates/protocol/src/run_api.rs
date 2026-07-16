//! Transport-neutral commands and projections for the canonical Run API.
//!
//! These DTOs deliberately do not expose [`RunRequest`](crate::agent_runtime::RunRequest).
//! Host-owned prompt, actor, provider, execution-fingerprint, and accounting
//! fields are composed by the application service rather than accepted from a
//! transport client.

use serde::{Deserialize, Serialize};

use crate::agent_runtime::{
    ModelAccounting, ReasoningEffort, RunId, RunLimits, StoredRuntimeEvent, TerminalState,
    ToolPolicy, Usage,
};

/// Current schema version for Run API command and response envelopes.
pub const RUN_API_SCHEMA_VERSION: u32 = 1;

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
    #[serde(default)]
    pub tool_policy: ToolPolicy,
    #[serde(default)]
    pub limits: RunLimits,
    #[serde(default)]
    pub controls: RunProductControls,
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
}

/// Read-only projection of one canonical run.
///
/// Every field is derived from the existing Agent runtime request/snapshot;
/// this type does not introduce another lifecycle or terminal status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunView {
    pub run_id: RunId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<RunId>,
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

/// Stable machine-readable error codes shared by HTTP and stdio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunApiErrorCode {
    InvalidRequest,
    RunAlreadyExists,
    RunNotFound,
    RunAlreadyRunning,
    RunNotActive,
    RunTerminal,
    RunEnvironmentMismatch,
    EventCursorAhead,
    RunStoreFailed,
}

/// Typed Run API failure. Human-readable text is supplementary to `code`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunApiError {
    pub code: RunApiErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalState>,
}

/// Transport-neutral result payload for every Run command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunCommandResult {
    Run {
        run: RunView,
    },
    Events {
        run_id: RunId,
        after_sequence: u64,
        events: Vec<StoredRuntimeEvent>,
    },
    Accepted {
        run_id: RunId,
        last_sequence: u64,
    },
    Error {
        error: RunApiError,
    },
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
    use crate::agent_runtime::{RuntimeEventId, RuntimeEventKind, StoredRuntimeEvent, ToolPolicy};

    fn start_command() -> StartRunCommand {
        StartRunCommand {
            input: "修复边界错误".to_owned(),
            workspace: "/workspace/project".to_owned(),
            model: Some("deepseek-v4-flash".to_owned()),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: Some(8_192),
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
                sandbox: Some("workspace_write".to_owned()),
            },
        }
    }

    fn run_view() -> RunView {
        RunView {
            run_id: RunId::from("run-1"),
            parent_run_id: None,
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
                "schema_version": 1,
                "request_id": "request-1",
                "command": {
                    "kind": "start",
                    "input": "修复边界错误",
                    "workspace": "/workspace/project",
                    "model": "deepseek-v4-flash",
                    "reasoning_effort": "high",
                    "max_output_tokens": 8192,
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
            "tool_catalog_sha256",
            "execution_fingerprint_sha256",
            "accounting_baseline",
            "transcript",
            "parent_run_id",
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
            RunCommand::Get {
                run_id: RunId::from("run-1"),
            },
            RunCommand::Events {
                run_id: RunId::from("run-1"),
                after_sequence: 7,
            },
            RunCommand::Resume {
                run_id: RunId::from("run-1"),
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
        ];
        let expected = [
            "start",
            "get",
            "events",
            "resume",
            "steer",
            "interrupt",
            "cancel",
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
    fn response_uses_existing_run_view_and_stored_event_envelope() {
        let event = StoredRuntimeEvent {
            schema_version: 3,
            run_id: RunId::from("run-1"),
            parent_run_id: None,
            event_id: RuntimeEventId("steer-1".to_owned()),
            sequence: 2,
            occurred_at_unix_ms: 123,
            event: RuntimeEventKind::Steered {
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
            result: RunCommandResult::Run { run: run_view() },
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
    fn error_codes_have_stable_snake_case_values() {
        let cases = [
            (RunApiErrorCode::InvalidRequest, "invalid_request"),
            (RunApiErrorCode::RunAlreadyExists, "run_already_exists"),
            (RunApiErrorCode::RunNotFound, "run_not_found"),
            (RunApiErrorCode::RunAlreadyRunning, "run_already_running"),
            (RunApiErrorCode::RunNotActive, "run_not_active"),
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
