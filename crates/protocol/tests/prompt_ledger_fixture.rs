use std::collections::HashMap;

use codewhale_protocol::agent_runtime::{
    AGENT_RUNTIME_EVENT_SCHEMA_VERSION, AgentActorKind, ModelRequest, ModelRetryDecision,
    PromptCacheControl, RunId, RuntimeEventKind, StoredRuntimeEvent, SystemPrompt,
};

const FIXTURE: &str =
    include_str!("../../../eval/fixtures/deepseek-exec/runtime-event-v6-prompt-ledger.json");

fn stable_prefix(prompt: &SystemPrompt) -> Vec<&str> {
    prompt
        .blocks
        .iter()
        .take_while(|block| block.cache_control == PromptCacheControl::Stable)
        .map(|block| block.text.as_str())
        .collect()
}

fn assert_retry_only_advances_attempt(initial: &ModelRequest, retry: &ModelRequest) {
    assert_eq!(retry.request_number, initial.request_number);
    assert_eq!(retry.attempt, initial.attempt + 1);

    let mut normalized_retry = retry.clone();
    normalized_retry.attempt = initial.attempt;
    assert_eq!(&normalized_retry, initial);
}

#[test]
fn runtime_event_v6_prompt_ledger_fixture_matches_rust_contract() {
    let events: Vec<StoredRuntimeEvent> =
        serde_json::from_str(FIXTURE).expect("fixture must use the Rust RuntimeEvent v6 schema");
    assert!(!events.is_empty());

    for event in &events {
        assert_eq!(
            event.schema_version, AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            "fixture must remain pinned to the current writer schema"
        );
        let normalized =
            serde_json::to_vec(event).expect("StoredRuntimeEvent must serialize canonically");
        let decoded: StoredRuntimeEvent =
            serde_json::from_slice(&normalized).expect("canonical event must deserialize");
        assert_eq!(
            &decoded, event,
            "canonical serde round-trip changed the event"
        );
    }

    let mut runs: HashMap<&RunId, Vec<&StoredRuntimeEvent>> = HashMap::new();
    for event in &events {
        runs.entry(&event.run_id).or_default().push(event);
    }
    assert_eq!(runs.len(), 2, "fixture must contain one root and one child");

    let root_id = RunId::from("fixture-root-run");
    let child_id = RunId::from("fixture-child-run");
    let root = runs.get(&root_id).expect("root run must exist");
    let child = runs.get(&child_id).expect("child run must exist");

    for run in [root, child] {
        let sequences = run.iter().map(|event| event.sequence).collect::<Vec<_>>();
        assert_eq!(
            sequences,
            (1..=run.len() as u64).collect::<Vec<_>>(),
            "each run must have a gap-free sequence"
        );
        assert!(
            matches!(
                run.last().map(|event| &event.event),
                Some(RuntimeEventKind::Terminal { .. })
            ),
            "terminal must be the last event in each run"
        );
        assert_eq!(
            run.iter()
                .filter(|event| matches!(event.event, RuntimeEventKind::Terminal { .. }))
                .count(),
            1,
            "each run must contain exactly one terminal"
        );
    }

    let root_base_prompt = match &root[0].event {
        RuntimeEventKind::RunCreated { request } => {
            assert_eq!(request.run_id.as_ref(), Some(&root_id));
            assert_eq!(request.parent_run_id, None);
            assert_eq!(request.actor.kind, AgentActorKind::Root);
            assert_eq!(request.actor.depth, 0);
            &request.system_prompt
        }
        event => panic!("root sequence 1 must be RunCreated, got {event:?}"),
    };
    let child_base_prompt = match &child[0].event {
        RuntimeEventKind::RunCreated { request } => {
            assert_eq!(request.run_id.as_ref(), Some(&child_id));
            assert_eq!(request.parent_run_id.as_ref(), Some(&root_id));
            assert_eq!(request.actor.kind, AgentActorKind::Child);
            assert_eq!(request.actor.depth, 1);
            &request.system_prompt
        }
        event => panic!("child sequence 1 must be RunCreated, got {event:?}"),
    };
    assert_eq!(
        stable_prefix(root_base_prompt),
        stable_prefix(child_base_prompt),
        "root and child base prompts must share the Agent stable prefix"
    );
    assert!(root.iter().all(|event| event.parent_run_id.is_none()));
    assert!(
        child
            .iter()
            .all(|event| event.parent_run_id.as_ref() == Some(&root_id))
    );

    let (agent_prepared_sequence, agent_attempt, agent_request) = root
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ModelRequestPrepared {
                attempt_id,
                request,
            } => Some((stored.sequence, attempt_id, request.as_ref())),
            _ => None,
        })
        .expect("fixture must contain an ordinary prepared model request");
    let agent_in_flight_sequence = root
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ModelRequestInFlight { attempt_id }
                if attempt_id == agent_attempt =>
            {
                Some(stored.sequence)
            }
            _ => None,
        })
        .expect("ordinary prepared request must become in-flight");
    let (agent_failed_sequence, agent_retry_attempt, agent_retry_request) = root
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ModelRequestFailed {
                attempt_id,
                retry: ModelRetryDecision::Retry { prepared },
                ..
            } if attempt_id == agent_attempt => Some((
                stored.sequence,
                &prepared.attempt_id,
                prepared.request.as_ref(),
            )),
            _ => None,
        })
        .expect("ordinary failure must atomically prepare a retry");
    assert!(agent_prepared_sequence < agent_in_flight_sequence);
    assert!(agent_in_flight_sequence < agent_failed_sequence);
    assert_retry_only_advances_attempt(agent_request, agent_retry_request);
    let agent_retry_in_flight_sequence = root
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ModelRequestInFlight { attempt_id }
                if attempt_id == agent_retry_attempt =>
            {
                Some(stored.sequence)
            }
            _ => None,
        })
        .expect("ordinary prepared retry must become in-flight");
    assert!(agent_failed_sequence < agent_retry_in_flight_sequence);

    let (compaction_prepared_sequence, compaction_attempt, compaction_request) = child
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ContextCompactionPrepared {
                attempt_id,
                request,
                ..
            } => Some((stored.sequence, attempt_id, request.as_ref())),
            _ => None,
        })
        .expect("fixture must contain a prepared compaction request");
    let compaction_in_flight_sequence = child
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ContextCompactionInFlight { attempt_id, .. }
                if attempt_id == compaction_attempt =>
            {
                Some(stored.sequence)
            }
            _ => None,
        })
        .expect("prepared compaction request must become in-flight");
    let (compaction_failed_sequence, compaction_retry_attempt, compaction_retry_request) = child
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ContextCompactionAttemptFailed {
                attempt_id,
                retry: ModelRetryDecision::Retry { prepared },
                ..
            } if attempt_id == compaction_attempt => Some((
                stored.sequence,
                &prepared.attempt_id,
                prepared.request.as_ref(),
            )),
            _ => None,
        })
        .expect("compaction failure must atomically prepare a retry");
    assert!(compaction_prepared_sequence < compaction_in_flight_sequence);
    assert!(compaction_in_flight_sequence < compaction_failed_sequence);
    assert_retry_only_advances_attempt(compaction_request, compaction_retry_request);
    let compaction_retry_in_flight_sequence = child
        .iter()
        .find_map(|stored| match &stored.event {
            RuntimeEventKind::ContextCompactionInFlight { attempt_id, .. }
                if attempt_id == compaction_retry_attempt =>
            {
                Some(stored.sequence)
            }
            _ => None,
        })
        .expect("prepared compaction retry must become in-flight");
    assert!(compaction_failed_sequence < compaction_retry_in_flight_sequence);
    assert_ne!(
        stable_prefix(&agent_request.system_prompt),
        stable_prefix(&compaction_request.system_prompt),
        "bounded compaction has its own stable system prompt"
    );
}
