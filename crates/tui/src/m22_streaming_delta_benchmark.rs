use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use dse_app::{AgentApplication, ProductionApplicationConfig};
use dse_context::compaction::{ContextInput, effective_context};
use dse_protocol::agent_runtime::{
    AgentActor, AttemptId, CanonicalTranscript, ContextPolicy, ModelAccounting, ModelRequest,
    ModelRouteAudit, ModelRouteProfile, PendingRuntimeEvent, ReasoningEffort, RunEnvironment,
    RunId, RunLimits, RunRequest, RuntimeEventId, RuntimeEventKind, SystemPrompt, ToolPolicy,
};
use dse_protocol::run_api::{
    RUN_API_SCHEMA_VERSION, RunCommand, RunCommandEnvelope, RunCommandResult,
};
use dse_protocol::task::{TaskContract, TaskDefinition, TaskGenerationId};
use dse_runtime::{CreatedRun, RunStore};
use dse_state::StateStore;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use crate::tui::run_projection::CanonicalRunProjection;
use crate::{ExecStreamEvent, exec_stream_line};

const FIXTURE: &str = include_str!("../../../eval/fixtures/m22-streaming-delta-baseline-v1.json");
const CANDIDATE_COUNTS_ENV: &str = "DSE_M22_CANDIDATE_COUNTS_JSON";

#[derive(Debug, Deserialize)]
struct Fixture {
    profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Deserialize)]
struct Profile {
    id: String,
    observed_reasoning_delta_events: usize,
    observed_reasoning_delta_utf8_bytes: usize,
    observed_content_delta_events: usize,
    observed_content_delta_utf8_bytes: usize,
    synthetic_total_events: usize,
}

#[derive(Debug, Deserialize)]
struct CandidateCountsInput {
    schema: String,
    profiles: Vec<CandidateProfileCounts>,
}

#[derive(Debug, Clone, Deserialize)]
struct CandidateProfileCounts {
    id: String,
    reasoning_delta_events: usize,
    content_delta_events: usize,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    schema: &'static str,
    warmups_per_profile: usize,
    measured_repetitions_per_profile: usize,
    profiles: Vec<ProfileReport>,
}

#[derive(Debug, Serialize)]
struct ProfileReport {
    id: String,
    reasoning_delta_events: usize,
    reasoning_delta_utf8_bytes: usize,
    content_delta_events: usize,
    content_delta_utf8_bytes: usize,
    synthetic_total_events: usize,
    samples: Vec<Sample>,
    candidate_reasoning_delta_events: usize,
    candidate_content_delta_events: usize,
    candidate_synthetic_total_events: usize,
    candidate_samples: Vec<Sample>,
}

#[derive(Debug, Serialize)]
struct Sample {
    delta_append_us: u64,
    credential_free_reopen_us: u64,
    run_api_events_json_us: u64,
    tui_projection_us: u64,
    headless_projection_us: u64,
    sqlite_plus_wal_bytes: u64,
    canonical_json_bytes: usize,
    projected_effects: usize,
    headless_bytes: usize,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "M22 deterministic local benchmark; run through scripts/eval-m22-streaming-delta-baseline.py"]
async fn canonical_m22_streaming_delta_baseline() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("M22 fixture must decode");
    let candidate_counts = candidate_counts();
    let warmups = 1;
    let repetitions = 5;
    let mut profiles = Vec::new();

    for profile in fixture.profiles {
        let mut samples = Vec::new();
        let mut candidate_samples = Vec::new();
        let candidate = candidate_counts
            .get(&profile.id)
            .expect("M22 candidate profile must match fixture");
        for repetition in 0..(warmups + repetitions) {
            let sample = benchmark_profile(
                &profile,
                repetition,
                "baseline",
                profile.observed_reasoning_delta_events,
                profile.observed_content_delta_events,
            )
            .await;
            let candidate_sample = benchmark_profile(
                &profile,
                repetition,
                "candidate",
                candidate.reasoning_delta_events,
                candidate.content_delta_events,
            )
            .await;
            if repetition >= warmups {
                samples.push(sample);
                candidate_samples.push(candidate_sample);
            }
        }
        profiles.push(ProfileReport {
            id: profile.id.clone(),
            reasoning_delta_events: profile.observed_reasoning_delta_events,
            reasoning_delta_utf8_bytes: profile.observed_reasoning_delta_utf8_bytes,
            content_delta_events: profile.observed_content_delta_events,
            content_delta_utf8_bytes: profile.observed_content_delta_utf8_bytes,
            synthetic_total_events: profile.synthetic_total_events,
            samples,
            candidate_reasoning_delta_events: candidate.reasoning_delta_events,
            candidate_content_delta_events: candidate.content_delta_events,
            candidate_synthetic_total_events: candidate
                .reasoning_delta_events
                .saturating_add(candidate.content_delta_events)
                .saturating_add(3),
            candidate_samples,
        });
    }

    let report = BenchmarkReport {
        schema: "dse.eval.m22-streaming-delta-rust-benchmark.v2",
        warmups_per_profile: warmups,
        measured_repetitions_per_profile: repetitions,
        profiles,
    };
    println!(
        "M22_BENCHMARK_JSON={}",
        serde_json::to_string(&report).expect("M22 benchmark report must encode")
    );
}

fn candidate_counts() -> BTreeMap<String, CandidateProfileCounts> {
    let raw = std::env::var(CANDIDATE_COUNTS_ENV)
        .expect("M22 harness must provide candidate transport counts");
    let input: CandidateCountsInput =
        serde_json::from_str(&raw).expect("M22 candidate counts must decode");
    assert_eq!(
        input.schema,
        "dse.eval.m22-streaming-delta-candidate-counts.v1"
    );
    input
        .profiles
        .into_iter()
        .map(|profile| {
            assert!(profile.reasoning_delta_events > 0);
            assert!(profile.content_delta_events > 0);
            (profile.id.clone(), profile)
        })
        .collect()
}

async fn benchmark_profile(
    profile: &Profile,
    repetition: usize,
    variant: &str,
    reasoning_delta_events: usize,
    content_delta_events: usize,
) -> Sample {
    let temp = TempDir::new().expect("M22 benchmark temp dir");
    let db_path = temp.path().join(format!("state-{variant}-{repetition}.db"));
    let workspace = temp
        .path()
        .canonicalize()
        .expect("canonical M22 workspace")
        .to_string_lossy()
        .into_owned();
    let run_id = RunId::from(format!("m22-{variant}-{}-{repetition}", profile.id));
    let attempt_id = AttemptId(format!("attempt-{variant}-{}-{repetition}", profile.id));
    let store = StateStore::open(Some(db_path.clone())).expect("open M22 StateStore");
    let created = store
        .create(run_request(&run_id, &workspace))
        .await
        .expect("create M22 run");
    store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId(format!("prepared-{repetition}")),
                event: RuntimeEventKind::ModelRequestPrepared {
                    attempt_id: attempt_id.clone(),
                    request: Box::new(model_request(&created)),
                },
            },
        )
        .await
        .expect("prepare M22 model request");
    store
        .append(
            &created.lease,
            PendingRuntimeEvent {
                event_id: RuntimeEventId(format!("in-flight-{repetition}")),
                event: RuntimeEventKind::ModelRequestInFlight {
                    attempt_id: attempt_id.clone(),
                },
            },
        )
        .await
        .expect("start M22 model request");

    let append_started = Instant::now();
    append_deltas(
        &store,
        &created.lease,
        &attempt_id,
        "reasoning",
        reasoning_delta_events,
        profile.observed_reasoning_delta_utf8_bytes,
        true,
    )
    .await;
    append_deltas(
        &store,
        &created.lease,
        &attempt_id,
        "content",
        content_delta_events,
        profile.observed_content_delta_utf8_bytes,
        false,
    )
    .await;
    let delta_append_us = elapsed_us(append_started);

    let replay = store
        .load(&run_id)
        .await
        .expect("load M22 run")
        .expect("M22 run must exist");
    let synthetic_total_events = reasoning_delta_events
        .saturating_add(content_delta_events)
        .saturating_add(3);
    assert_eq!(replay.events.len(), synthetic_total_events);
    assert_delta_identity(
        &replay.events,
        profile,
        reasoning_delta_events,
        content_delta_events,
    );
    drop(replay);
    drop(store);

    let sqlite_plus_wal_bytes = sqlite_footprint(&db_path);
    let reopen_started = Instant::now();
    let reopened = StateStore::open(Some(db_path.clone())).expect("reopen M22 StateStore");
    let replay = reopened
        .load(&run_id)
        .await
        .expect("load reopened M22 run")
        .expect("reopened M22 run must exist");
    let credential_free_reopen_us = elapsed_us(reopen_started);
    assert_eq!(replay.events.len(), synthetic_total_events);
    assert_delta_identity(
        &replay.events,
        profile,
        reasoning_delta_events,
        content_delta_events,
    );

    let tui_started = Instant::now();
    let mut projection = CanonicalRunProjection::new();
    let mut projected_effects = 0;
    for event in &replay.events {
        projected_effects += projection
            .apply(event.clone())
            .expect("TUI projection must accept M22 event")
            .len();
    }
    let tui_projection_us = elapsed_us(tui_started);

    let headless_started = Instant::now();
    let mut headless_bytes = 0;
    for event in &replay.events {
        if let RuntimeEventKind::ContentDelta { delta, .. } = &event.event {
            headless_bytes += exec_stream_line(&ExecStreamEvent::Content {
                content: delta.clone(),
            })
            .expect("headless content delta must serialize")
            .len();
        }
    }
    let headless_projection_us = elapsed_us(headless_started);
    drop(reopened);
    drop(replay);

    let application = AgentApplication::production(
        ProductionApplicationConfig::official().with_state_db_path(&db_path),
    )
    .expect("open production application without credential");
    let api_started = Instant::now();
    let response = application
        .execute(RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: format!("m22-events-{}-{repetition}", profile.id),
            command: RunCommand::Events {
                run_id: run_id.clone(),
                after_sequence: 0,
            },
        })
        .await;
    let event_count = match &response.result {
        RunCommandResult::Events { events, .. } => events.len(),
        other => panic!("M22 Run API returned {other:?}"),
    };
    let canonical_json =
        serde_json::to_vec(&response).expect("canonical Run API response must serialize");
    let run_api_events_json_us = elapsed_us(api_started);
    assert_eq!(event_count, synthetic_total_events);

    Sample {
        delta_append_us,
        credential_free_reopen_us,
        run_api_events_json_us,
        tui_projection_us,
        headless_projection_us,
        sqlite_plus_wal_bytes,
        canonical_json_bytes: canonical_json.len(),
        projected_effects,
        headless_bytes,
    }
}

async fn append_deltas(
    store: &StateStore,
    lease: &dse_runtime::RunLease,
    attempt_id: &AttemptId,
    label: &str,
    count: usize,
    total_bytes: usize,
    reasoning: bool,
) {
    assert!(count > 0);
    assert!(total_bytes >= count);
    let base = total_bytes / count;
    let remainder = total_bytes % count;
    for offset in 0..count {
        let length = base + usize::from(offset < remainder);
        let delta = if reasoning {
            "r".repeat(length)
        } else {
            "c".repeat(length)
        };
        let index = u64::try_from(offset + 1).expect("M22 delta index fits u64");
        let event = if reasoning {
            RuntimeEventKind::ReasoningDelta {
                attempt_id: attempt_id.clone(),
                index,
                delta,
            }
        } else {
            RuntimeEventKind::ContentDelta {
                attempt_id: attempt_id.clone(),
                index,
                delta,
            }
        };
        store
            .append(
                lease,
                PendingRuntimeEvent {
                    event_id: RuntimeEventId(format!("{label}-{offset}")),
                    event,
                },
            )
            .await
            .expect("append M22 streaming delta");
    }
}

fn run_request(run_id: &RunId, workspace: &str) -> RunRequest {
    RunRequest {
        run_id: Some(run_id.clone()),
        parent_run_id: None,
        continued_from_run_id: None,
        model: "deepseek-v4-pro".to_owned(),
        route: ModelRouteAudit {
            profile: ModelRouteProfile::Explicit,
            policy_version: "m22_fixed_pro_v1".to_owned(),
            reason_code: "m22_benchmark".to_owned(),
        },
        task_contract: Some(TaskContract {
            generation_id: TaskGenerationId::from(run_id.0.clone()),
            definition: TaskDefinition::host("量化 canonical streaming delta 写放大"),
        }),
        system_prompt: SystemPrompt::from("M22 deterministic streaming benchmark"),
        transcript: CanonicalTranscript::default(),
        reasoning_effort: ReasoningEffort::High,
        max_output_tokens: Some(8_192),
        streaming: true,
        actor: AgentActor::default(),
        agent_task: None,
        deadline_unix_ms: None,
        tool_policy: ToolPolicy::default(),
        limits: RunLimits::default(),
        environment: RunEnvironment {
            workspace: workspace.to_owned(),
            provider: "deepseek".to_owned(),
            ..RunEnvironment::default()
        },
        context_policy: ContextPolicy::default(),
        context_projection: None,
        inherited_facts: None,
        accounting_baseline: ModelAccounting::default(),
    }
}

fn model_request(created: &CreatedRun) -> ModelRequest {
    let snapshot = &created.replay.snapshot;
    let context = effective_context(ContextInput {
        transcript: &snapshot.transcript,
        projection: snapshot.context_projection.as_ref(),
        task_contract: snapshot.request.task_contract.as_ref(),
        workspace_state: &snapshot.workspace_state,
        evidence_receipts: &snapshot.evidence_receipts,
        last_completion_rejection: snapshot.last_completion_rejection.as_ref(),
        last_verifier_failure: snapshot
            .last_host_verification_failure
            .as_ref()
            .map(|failure| &failure.outcome),
        last_verifier_failure_workspace: snapshot
            .last_host_verification_failure
            .as_ref()
            .map(|failure| &failure.workspace_state),
        tools: &[],
    })
    .expect("build canonical M22 model request projection");
    ModelRequest {
        run_id: created.lease.run_id.clone(),
        parent_run_id: snapshot.request.parent_run_id.clone(),
        actor: snapshot.request.actor,
        model: snapshot.request.model.clone(),
        system_prompt: context.system_prompt,
        messages: context.messages,
        tools: Vec::new(),
        reasoning_effort: snapshot.request.reasoning_effort,
        max_output_tokens: snapshot.request.max_output_tokens,
        streaming: snapshot.request.streaming,
        request_number: snapshot.local_turns.saturating_add(1),
        attempt: 0,
    }
}

fn assert_delta_identity(
    events: &[dse_protocol::agent_runtime::StoredRuntimeEvent],
    profile: &Profile,
    expected_reasoning_events: usize,
    expected_content_events: usize,
) {
    let mut reasoning_count = 0;
    let mut reasoning_bytes = 0;
    let mut reasoning = String::new();
    let mut content_count = 0;
    let mut content_bytes = 0;
    let mut content = String::new();
    for event in events {
        match &event.event {
            RuntimeEventKind::ReasoningDelta { delta, .. } => {
                reasoning_count += 1;
                reasoning_bytes += delta.len();
                reasoning.push_str(delta);
            }
            RuntimeEventKind::ContentDelta { delta, .. } => {
                content_count += 1;
                content_bytes += delta.len();
                content.push_str(delta);
            }
            _ => {}
        }
    }
    assert_eq!(reasoning_count, expected_reasoning_events);
    assert_eq!(reasoning_bytes, profile.observed_reasoning_delta_utf8_bytes);
    assert_eq!(content_count, expected_content_events);
    assert_eq!(content_bytes, profile.observed_content_delta_utf8_bytes);
    assert_eq!(reasoning.len(), reasoning_bytes);
    assert_eq!(content.len(), content_bytes);
}

fn sqlite_footprint(path: &Path) -> u64 {
    ["", "-wal", "-shm"]
        .into_iter()
        .map(|suffix| {
            let candidate = PathBuf::from(format!("{}{suffix}", path.display()));
            std::fs::metadata(candidate)
                .map(|metadata| metadata.len())
                .unwrap_or(0)
        })
        .sum()
}

fn elapsed_us(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}
