//! Evidence-aware, deterministic projection for the single Agent runtime.
//!
//! The canonical transcript and RunStore facts remain the only durable truth.
//! This module is a pure request projector: it keeps mandatory Host facts,
//! selects complete reasoning/tool groups, and commits only provenance-backed
//! model-visible projections. It never calls a model or reads the workspace.

use std::collections::BTreeSet;

use codewhale_protocol::agent_runtime::{
    CanonicalTranscript, ContextPolicy, ContextProjection, ModelMessage, ToolDefinition,
    ToolSideEffectStatus, TranscriptEntry,
};
use codewhale_protocol::task::{
    CompletionRejection, EvidenceReceipt, TaskAcceptance, TaskContract, WorkspaceRevision,
    WorkspaceState,
};
use sha2::{Digest, Sha256};

const TOOL_RESULT_PRUNE_CHARS: usize = 16 * 1024;
const TOOL_RESULT_RETAIN_CHARS: usize = 2 * 1024;
const HOST_FACTS_HEADER: &str = "## 当前 Host 事实";
const LIMIT_TARGET_NUMERATOR: u64 = 3;
const LIMIT_TARGET_DENOMINATOR: u64 = 4;

/// Narrow, borrowed view of the canonical facts needed for one model request.
///
/// Presentation clients never construct this value. AgentRuntime and the
/// RunStore reducer both derive it from the same durable snapshot.
#[derive(Debug, Clone, Copy)]
pub struct ContextInput<'a> {
    pub transcript: &'a CanonicalTranscript,
    pub projection: Option<&'a ContextProjection>,
    pub task_contract: Option<&'a TaskContract>,
    pub workspace_state: &'a WorkspaceState,
    pub evidence_receipts: &'a [EvidenceReceipt],
    pub last_completion_rejection: Option<&'a CompletionRejection>,
    pub last_verifier_failure: Option<&'a codewhale_protocol::agent_runtime::ToolOutcome>,
    pub last_verifier_failure_workspace: Option<&'a WorkspaceState>,
    pub tools: &'a [ToolDefinition],
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveContext {
    pub system_prompt: codewhale_protocol::agent_runtime::SystemPrompt,
    pub messages: Vec<ModelMessage>,
    /// Canonical transcript indices represented by the history portion of
    /// `messages`. The deterministic Host-facts tail has no transcript index.
    pub source_entry_indices: Vec<u64>,
    pub source_entry_count: u64,
    pub sha256: String,
    pub estimated_tokens: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContextCompactionPreparation {
    NotNeeded {
        estimated_tokens: u64,
    },
    Local {
        projection: ContextProjection,
        before_tokens: u64,
        after_tokens: u64,
    },
    LimitExceeded {
        estimated_tokens: u64,
        hard_input_tokens: u64,
    },
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ContextProjectionError {
    #[error(
        "context projection boundary {boundary} is beyond canonical transcript length {entries}"
    )]
    BoundaryAhead { boundary: u64, entries: usize },
    #[error("context projection source indices are invalid")]
    InvalidSourceIndices,
    #[error("context projection messages do not match their canonical sources")]
    SourceMessageMismatch,
    #[error("failed to serialize context facts for a stable digest: {0}")]
    Digest(String),
    #[error("invalid context policy: {0}")]
    InvalidPolicy(String),
}

/// Materialize the exact model-visible request context.
pub fn effective_context(
    input: ContextInput<'_>,
) -> Result<EffectiveContext, ContextProjectionError> {
    let system_prompt = input
        .transcript
        .entries
        .iter()
        .find_map(|entry| match entry {
            TranscriptEntry::System { prompt } => Some(prompt.clone()),
            _ => None,
        })
        .unwrap_or_default();

    let (mut messages, mut source_entry_indices) = match input.projection {
        Some(projection) => projected_history(input.transcript, projection)?,
        None => project_range(input.transcript, 0, input.transcript.entries.len(), false),
    };

    if let Some(host_facts) = render_host_facts(input, &messages)? {
        messages.push(ModelMessage::User {
            content: host_facts,
        });
    }

    let source_entry_count = u64::try_from(input.transcript.entries.len()).unwrap_or(u64::MAX);
    let sha256 = projection_digest(
        &system_prompt,
        &messages,
        &source_entry_indices,
        source_entry_count,
        input.tools,
    )?;
    let estimated_tokens = estimate_context_tokens(&system_prompt, &messages, input.tools);

    // Keep the allocation owned by the returned bundle and make it explicit
    // that Host facts do not manufacture canonical transcript provenance.
    source_entry_indices.shrink_to_fit();
    Ok(EffectiveContext {
        system_prompt,
        messages,
        source_entry_indices,
        source_entry_count,
        sha256,
        estimated_tokens,
    })
}

/// Plan one deterministic local compaction.
///
/// Mandatory facts are never semantically summarized. If they alone exceed
/// the hard budget, the caller receives a typed limit outcome before any
/// DeepSeek request is admitted.
pub fn prepare_compaction(
    input: ContextInput<'_>,
    policy: ContextPolicy,
) -> Result<ContextCompactionPreparation, ContextProjectionError> {
    validate_policy(policy)?;
    let effective = effective_context(input)?;
    let hard = u64::from(policy.hard_input_tokens);
    if effective.estimated_tokens <= hard {
        return Ok(ContextCompactionPreparation::NotNeeded {
            estimated_tokens: effective.estimated_tokens,
        });
    }

    let mandatory = mandatory_entry_indices(input);
    let groups = atomic_history_groups(input.transcript);
    let mut selected = mandatory;
    expand_selected_groups(&mut selected, &groups);

    let mut candidate = projection_from_indices(&effective, input.transcript, &selected);
    let mut after_tokens = estimate_projection_tokens(input, &candidate)?;
    if after_tokens > hard {
        return Ok(ContextCompactionPreparation::LimitExceeded {
            estimated_tokens: after_tokens,
            hard_input_tokens: hard,
        });
    }
    let selection_target = hard
        .min(
            effective
                .estimated_tokens
                .saturating_mul(LIMIT_TARGET_NUMERATOR)
                / LIMIT_TARGET_DENOMINATOR,
        )
        .max(after_tokens);

    // Spend the remaining budget on newest complete groups. A reasoning/tool
    // group is admitted whole or not at all.
    for group in groups.iter().rev() {
        if group.iter().all(|index| selected.contains(index)) {
            continue;
        }
        let mut trial = selected.clone();
        trial.extend(group.iter().copied());
        let trial_projection = projection_from_indices(&effective, input.transcript, &trial);
        let trial_tokens = estimate_projection_tokens(input, &trial_projection)?;
        if trial_tokens <= selection_target {
            selected = trial;
            candidate = trial_projection;
            after_tokens = trial_tokens;
        }
    }

    if after_tokens >= effective.estimated_tokens {
        return Ok(ContextCompactionPreparation::LimitExceeded {
            estimated_tokens: effective.estimated_tokens,
            hard_input_tokens: hard,
        });
    }

    Ok(ContextCompactionPreparation::Local {
        projection: candidate,
        before_tokens: effective.estimated_tokens,
        after_tokens,
    })
}

pub fn estimate_projection_tokens(
    input: ContextInput<'_>,
    projection: &ContextProjection,
) -> Result<u64, ContextProjectionError> {
    Ok(effective_context(ContextInput {
        projection: Some(projection),
        ..input
    })?
    .estimated_tokens)
}

fn projected_history(
    transcript: &CanonicalTranscript,
    projection: &ContextProjection,
) -> Result<(Vec<ModelMessage>, Vec<u64>), ContextProjectionError> {
    let boundary = usize::try_from(projection.source_entry_count).unwrap_or(usize::MAX);
    if boundary > transcript.entries.len() {
        return Err(ContextProjectionError::BoundaryAhead {
            boundary: projection.source_entry_count,
            entries: transcript.entries.len(),
        });
    }
    if projection.selected_entry_indices.len() != projection.messages.len()
        || !projection
            .selected_entry_indices
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        || projection
            .selected_entry_indices
            .iter()
            .any(|index| *index >= projection.source_entry_count)
    {
        return Err(ContextProjectionError::InvalidSourceIndices);
    }

    let selected = projection
        .selected_entry_indices
        .iter()
        .map(|index| usize::try_from(*index).unwrap_or(usize::MAX))
        .collect::<BTreeSet<_>>();
    let (expected, expected_indices) = project_selected(transcript, &selected, boundary, true);
    if expected != projection.messages || expected_indices != projection.selected_entry_indices {
        return Err(ContextProjectionError::SourceMessageMismatch);
    }

    let (suffix, suffix_indices) =
        project_range(transcript, boundary, transcript.entries.len(), false);
    let mut messages = projection.messages.clone();
    messages.extend(suffix);
    let mut source_indices = projection.selected_entry_indices.clone();
    source_indices.extend(suffix_indices);
    Ok((messages, source_indices))
}

fn projection_from_indices(
    source: &EffectiveContext,
    transcript: &CanonicalTranscript,
    selected: &BTreeSet<usize>,
) -> ContextProjection {
    let boundary = transcript.entries.len();
    let (messages, selected_entry_indices) = project_selected(transcript, selected, boundary, true);
    ContextProjection {
        source_entry_count: u64::try_from(boundary).unwrap_or(u64::MAX),
        source_projection_sha256: source.sha256.clone(),
        selected_entry_indices,
        messages,
    }
}

fn mandatory_entry_indices(input: ContextInput<'_>) -> BTreeSet<usize> {
    let entries = &input.transcript.entries;
    let task_index = input.task_contract.and_then(|contract| {
        let rendered = contract.definition.model_message();
        entries.iter().enumerate().rev().find_map(|(index, entry)| {
            matches!(entry, TranscriptEntry::User { content } if content == &rendered)
                .then_some(index)
        })
    });
    let task_start = task_index.unwrap_or(0);
    let mut selected = BTreeSet::new();
    if let Some(index) = task_index {
        selected.insert(index);
    }

    // Every current-task user steer/constraint and joined child handoff is a
    // durable semantic fact, not disposable chat prose.
    for (index, entry) in entries.iter().enumerate().skip(task_start) {
        if matches!(
            entry,
            TranscriptEntry::User { .. } | TranscriptEntry::ChildOutcome { .. }
        ) {
            selected.insert(index);
        }
    }

    // Preserve every current-task mutation group. Their tool calls and
    // outcomes are the canonical change facts when no later git_diff/status
    // exists; keeping only the last write would forget earlier files.
    let mutations = entries
        .iter()
        .enumerate()
        .skip(task_start)
        .filter_map(|(index, entry)| {
            let TranscriptEntry::Tool { outcome, .. } = entry else {
                return None;
            };
            (outcome.side_effect == ToolSideEffectStatus::Applied
                || outcome.workspace_revision.is_some())
            .then_some(index)
        })
        .collect::<Vec<_>>();
    selected.extend(mutations.iter().copied());
    let latest_mutation = mutations.last().copied();

    // A diff/status result is current only if no later mutation invalidated
    // it. The Broker performs no hidden Git I/O.
    for name in ["git_diff", "git_status"] {
        if let Some(index) = entries.iter().enumerate().rev().find_map(|(index, entry)| {
            matches!(entry, TranscriptEntry::Tool { name: tool, .. } if tool == name)
                .then_some(index)
        }) && latest_mutation.is_none_or(|mutation| index > mutation)
        {
            selected.insert(index);
        }
    }
    selected
}

fn atomic_history_groups(transcript: &CanonicalTranscript) -> Vec<Vec<usize>> {
    let entries = &transcript.entries;
    let mut groups = Vec::new();
    let mut cursor = 0;
    while cursor < entries.len() {
        match &entries[cursor] {
            TranscriptEntry::System { .. } => cursor += 1,
            TranscriptEntry::Assistant { tool_calls, .. } if !tool_calls.is_empty() => {
                let call_ids = tool_calls
                    .iter()
                    .map(|call| call.id.as_str())
                    .collect::<BTreeSet<_>>();
                let mut group = vec![cursor];
                let mut next = cursor + 1;
                while next < entries.len() {
                    match &entries[next] {
                        TranscriptEntry::Tool { call_id, .. }
                            if call_ids.contains(call_id.as_str()) =>
                        {
                            group.push(next);
                            next += 1;
                        }
                        TranscriptEntry::Tool { .. } | TranscriptEntry::ChildOutcome { .. } => {
                            next += 1;
                        }
                        _ => break,
                    }
                }
                groups.push(group);
                cursor = next;
            }
            TranscriptEntry::Tool { .. } => {
                // Orphan Tool entries are never selected as optional context.
                cursor += 1;
            }
            _ => {
                groups.push(vec![cursor]);
                cursor += 1;
            }
        }
    }
    groups
}

fn expand_selected_groups(selected: &mut BTreeSet<usize>, groups: &[Vec<usize>]) {
    loop {
        let mut changed = false;
        for group in groups {
            if group.iter().any(|index| selected.contains(index)) {
                for index in group {
                    changed |= selected.insert(*index);
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn project_selected(
    transcript: &CanonicalTranscript,
    selected: &BTreeSet<usize>,
    boundary: usize,
    compact_tools: bool,
) -> (Vec<ModelMessage>, Vec<u64>) {
    let mut messages = Vec::new();
    let mut indices = Vec::new();
    for index in selected.iter().copied().filter(|index| *index < boundary) {
        let Some(mut message) = project_entry(&transcript.entries[index]) else {
            continue;
        };
        if compact_tools {
            compact_tool_result(&mut message);
        }
        messages.push(message);
        indices.push(u64::try_from(index).unwrap_or(u64::MAX));
    }
    (messages, indices)
}

fn project_range(
    transcript: &CanonicalTranscript,
    start: usize,
    end: usize,
    compact_tools: bool,
) -> (Vec<ModelMessage>, Vec<u64>) {
    let mut messages = Vec::new();
    let mut indices = Vec::new();
    for index in start..end {
        let Some(mut message) = project_entry(&transcript.entries[index]) else {
            continue;
        };
        if compact_tools {
            compact_tool_result(&mut message);
        }
        messages.push(message);
        indices.push(u64::try_from(index).unwrap_or(u64::MAX));
    }
    (messages, indices)
}

fn project_entry(entry: &TranscriptEntry) -> Option<ModelMessage> {
    match entry {
        TranscriptEntry::System { .. } => None,
        TranscriptEntry::User { content } => Some(ModelMessage::User {
            content: content.clone(),
        }),
        TranscriptEntry::Assistant {
            content,
            reasoning_content,
            tool_calls,
        } => Some(ModelMessage::Assistant {
            content: content.clone(),
            reasoning_content: reasoning_content.clone(),
            tool_calls: tool_calls.clone(),
        }),
        TranscriptEntry::Tool {
            call_id,
            name,
            outcome,
        } => Some(ModelMessage::Tool {
            call_id: call_id.clone(),
            name: name.clone(),
            content: outcome.content.clone(),
        }),
        TranscriptEntry::ChildOutcome {
            handoff_content, ..
        } => Some(ModelMessage::User {
            content: handoff_content.clone(),
        }),
    }
}

fn compact_tool_result(message: &mut ModelMessage) {
    let ModelMessage::Tool { content, name, .. } = message else {
        return;
    };
    if content.chars().count() <= TOOL_RESULT_PRUNE_CHARS {
        return;
    }
    let original = content.chars().count();
    let head = take_chars(content, TOOL_RESULT_RETAIN_CHARS);
    let tail = take_tail_chars(content, TOOL_RESULT_RETAIN_CHARS);
    *content = format!(
        "[{name} 的旧工具结果已在请求投影中压缩，canonical 记录保留 {original} 字符]\n{head}\n…\n{tail}"
    );
}

fn render_host_facts(
    input: ContextInput<'_>,
    history: &[ModelMessage],
) -> Result<Option<String>, ContextProjectionError> {
    let mut lines = vec![HOST_FACTS_HEADER.to_owned()];

    if let Some(contract) = input.task_contract {
        let rendered = contract.definition.model_message();
        let already_present = history.iter().any(
            |message| matches!(message, ModelMessage::User { content } if content == &rendered),
        );
        if !already_present {
            lines.push("### 冻结任务契约".to_owned());
            lines.push(rendered);
        }
        let acceptance_ids = contract
            .definition
            .acceptance
            .iter()
            .map(|acceptance| acceptance.id().0.as_str())
            .collect::<Vec<_>>()
            .join(",");
        lines.push(format!(
            "- task_generation: `{}`\n- acceptance_ids: `{acceptance_ids}`",
            contract.generation_id.0
        ));
    }

    let revision = match &input.workspace_state.revision {
        WorkspaceRevision::Known { sha256 } => format!("known:{sha256}"),
        WorkspaceRevision::Unknown { reason } => format!("unknown:{reason}"),
    };
    lines.push(format!(
        "- workspace_generation: `{}`\n- workspace_revision: `{revision}`",
        input.workspace_state.generation
    ));

    if let Some(receipt) = latest_valid_receipt(input) {
        let encoded = serde_json::to_string(receipt)
            .map_err(|error| ContextProjectionError::Digest(error.to_string()))?;
        lines.push(format!("### 当前有效 EvidenceReceipt\n`{encoded}`"));
    } else {
        lines.push("- current_evidence_receipt: `none`".to_owned());
    }

    if let Some(rejection) = unresolved_rejection(input) {
        let encoded = serde_json::to_string(rejection)
            .map_err(|error| ContextProjectionError::Digest(error.to_string()))?;
        lines.push(format!("### 未解决的完成拒绝\n`{encoded}`"));
        if let Some(outcome) = input.last_verifier_failure {
            let mut outcome = outcome.clone();
            if outcome.content.chars().count() > TOOL_RESULT_PRUNE_CHARS {
                let original = outcome.content.chars().count();
                outcome.content = format!(
                    "[verifier 输出已确定性压缩，canonical 记录保留 {original} 字符]\n{}\n…\n{}",
                    take_chars(&outcome.content, TOOL_RESULT_RETAIN_CHARS * 3),
                    take_tail_chars(&outcome.content, TOOL_RESULT_RETAIN_CHARS),
                );
            }
            let encoded = serde_json::to_string(&outcome)
                .map_err(|error| ContextProjectionError::Digest(error.to_string()))?;
            lines.push(format!("### 最近一次 verifier 失败\n`{encoded}`"));
        }
        if let Some(workspace) = input.last_verifier_failure_workspace {
            let encoded = serde_json::to_string(workspace)
                .map_err(|error| ContextProjectionError::Digest(error.to_string()))?;
            lines.push(format!("- verifier_failure_workspace: `{encoded}`"));
        }
    }
    Ok(Some(lines.join("\n")))
}

fn latest_valid_receipt(input: ContextInput<'_>) -> Option<&EvidenceReceipt> {
    let contract = input.task_contract?;
    input.evidence_receipts.iter().rev().find(|receipt| {
        receipt.generation_id == contract.generation_id
            && receipt.workspace_state == *input.workspace_state
            && contract.definition.acceptance.iter().any(|acceptance| {
                matches!(
                    acceptance,
                    TaskAcceptance::Verifier { id, verifier, .. }
                        if *id == receipt.acceptance_id && *verifier == receipt.verifier
                )
            })
    })
}

fn unresolved_rejection(input: ContextInput<'_>) -> Option<&CompletionRejection> {
    let rejection = input.last_completion_rejection?;
    let contract = input.task_contract?;
    let resolved = rejection.unmet_acceptance_ids.iter().all(|acceptance_id| {
        input.evidence_receipts.iter().any(|receipt| {
            receipt.generation_id == contract.generation_id
                && receipt.acceptance_id == *acceptance_id
                && receipt.workspace_state == *input.workspace_state
                && contract.definition.acceptance.iter().any(|acceptance| {
                    matches!(
                        acceptance,
                        TaskAcceptance::Verifier { id, verifier, .. }
                            if id == acceptance_id && *verifier == receipt.verifier
                    )
                })
        })
    });
    (!resolved).then_some(rejection)
}

fn validate_policy(policy: ContextPolicy) -> Result<(), ContextProjectionError> {
    if policy.hard_input_tokens == 0 {
        return Err(ContextProjectionError::InvalidPolicy(
            "hard_input_tokens must be greater than zero".to_owned(),
        ));
    }
    Ok(())
}

fn projection_digest(
    system_prompt: &codewhale_protocol::agent_runtime::SystemPrompt,
    messages: &[ModelMessage],
    source_entry_indices: &[u64],
    source_entry_count: u64,
    tools: &[ToolDefinition],
) -> Result<String, ContextProjectionError> {
    let bytes = serde_json::to_vec(&(
        system_prompt,
        messages,
        source_entry_indices,
        source_entry_count,
        tools,
    ))
    .map_err(|error| ContextProjectionError::Digest(error.to_string()))?;
    Ok(format_sha256(&bytes))
}

fn estimate_context_tokens(
    system_prompt: &codewhale_protocol::agent_runtime::SystemPrompt,
    messages: &[ModelMessage],
    tools: &[ToolDefinition],
) -> u64 {
    let system_tokens = system_prompt
        .blocks
        .iter()
        .map(|block| estimate_text_tokens(&block.text))
        .sum::<usize>();
    let message_tokens = messages
        .iter()
        .map(|message| match message {
            ModelMessage::User { content } => estimate_text_tokens(content),
            ModelMessage::Assistant {
                content,
                reasoning_content,
                tool_calls,
            } => content
                .as_deref()
                .map_or(0, estimate_text_tokens)
                .saturating_add(reasoning_content.as_deref().map_or(0, estimate_text_tokens))
                .saturating_add(
                    tool_calls
                        .iter()
                        .map(|call| {
                            estimate_text_tokens(&call.id)
                                .saturating_add(estimate_text_tokens(&call.name))
                                .saturating_add(estimate_text_tokens(&call.arguments.raw))
                        })
                        .sum::<usize>(),
                ),
            ModelMessage::Tool {
                call_id,
                name,
                content,
            } => estimate_text_tokens(call_id)
                .saturating_add(estimate_text_tokens(name))
                .saturating_add(estimate_text_tokens(content)),
        })
        .sum::<usize>();
    let tool_tokens = tools
        .iter()
        .map(|tool| {
            estimate_text_tokens(&tool.name)
                .saturating_add(estimate_text_tokens(&tool.description))
                .saturating_add(estimate_text_tokens(&tool.input_schema.to_string()))
        })
        .sum::<usize>();
    let lexical_tokens = system_tokens
        .saturating_add(message_tokens)
        .saturating_add(tool_tokens);
    u64::try_from(lexical_tokens)
        .unwrap_or(u64::MAX)
        .saturating_add(
            u64::try_from(messages.len().saturating_add(tools.len()))
                .unwrap_or(u64::MAX)
                .saturating_mul(12),
        )
        .saturating_add(48)
}

/// Conservative tokenizer-free estimate calibrated for Chinese prose,
/// repetitive ASCII, and high-entropy identifiers.
fn estimate_text_tokens(value: &str) -> usize {
    let bytes = value.as_bytes();
    let mut tokens = 0usize;
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if !bytes[cursor].is_ascii() {
            let character = value[cursor..]
                .chars()
                .next()
                .expect("cursor is on a UTF-8 boundary");
            tokens = tokens.saturating_add(character.len_utf8().div_ceil(3));
            cursor = cursor.saturating_add(character.len_utf8());
            continue;
        }

        let start = cursor;
        let byte = bytes[cursor];
        if byte.is_ascii_whitespace() {
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            tokens = tokens.saturating_add((cursor - start).div_ceil(4));
            continue;
        }
        if byte.is_ascii_alphanumeric() {
            let mut distinct = [false; 128];
            let mut distinct_count = 0usize;
            let mut has_alpha = false;
            let mut has_digit = false;
            while cursor < bytes.len() && bytes[cursor].is_ascii_alphanumeric() {
                let current = bytes[cursor];
                let slot = &mut distinct[usize::from(current)];
                if !*slot {
                    *slot = true;
                    distinct_count += 1;
                }
                has_alpha |= current.is_ascii_alphabetic();
                has_digit |= current.is_ascii_digit();
                cursor += 1;
            }
            let length = cursor - start;
            let run_tokens = if has_digit && !has_alpha {
                length.div_ceil(2)
            } else if distinct_count == 1 && length >= 16 {
                length.div_ceil(6)
            } else if length >= 32 {
                length.saturating_mul(2).div_ceil(3)
            } else {
                length.div_ceil(4)
            };
            tokens = tokens.saturating_add(run_tokens);
            continue;
        }

        while cursor < bytes.len()
            && bytes[cursor].is_ascii()
            && !bytes[cursor].is_ascii_whitespace()
            && !bytes[cursor].is_ascii_alphanumeric()
        {
            cursor += 1;
        }
        let length = cursor - start;
        let repeated = bytes[start..cursor]
            .iter()
            .all(|current| *current == bytes[start]);
        tokens = tokens.saturating_add(if repeated { length.div_ceil(2) } else { length });
    }
    tokens
}

fn format_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{digest}")
}

fn take_chars(value: &str, count: usize) -> String {
    value.chars().take(count).collect()
}

fn take_tail_chars(value: &str, count: usize) -> String {
    let total = value.chars().count();
    value.chars().skip(total.saturating_sub(count)).collect()
}

#[cfg(test)]
mod tests {
    use codewhale_protocol::agent_runtime::{
        AgentOutcome, ModelAccounting, ModelToolCall, RunId, SystemPrompt, ToolArguments,
        ToolOutcome, TranscriptEntry,
    };
    use codewhale_protocol::task::{
        AcceptanceId, EvidenceReceiptId, TaskDefinition, TaskGenerationId, VerificationId,
        VerifierPlan, VerifierSpec, VerifierStep, WorkspaceRevision,
    };
    use serde_json::json;

    use super::*;

    fn contract() -> TaskContract {
        TaskContract {
            generation_id: TaskGenerationId::from("run-1"),
            definition: TaskDefinition {
                objective: "修复 sentinel 功能".to_owned(),
                constraints: vec!["必须保留 SENTINEL_CONSTRAINT".to_owned()],
                non_goals: vec!["不要修改测试".to_owned()],
                acceptance: vec![TaskAcceptance::Verifier {
                    id: AcceptanceId::from("accept-1"),
                    description: "通过冻结验证".to_owned(),
                    verifier: verifier(),
                }],
            },
        }
    }

    fn verifier() -> VerifierSpec {
        VerifierSpec {
            verifier_id: "run_verifiers".to_owned(),
            parameters: json!({"profile": "exact"}),
            plan: VerifierPlan {
                steps: vec![VerifierStep {
                    id: "check".to_owned(),
                    program: "cargo".to_owned(),
                    args: vec!["test".to_owned()],
                    cwd: String::new(),
                    env: Default::default(),
                    timeout_ms: 1_000,
                }],
            },
        }
    }

    fn workspace(generation: u64, revision: &str) -> WorkspaceState {
        WorkspaceState {
            generation,
            revision: WorkspaceRevision::Known {
                sha256: revision.to_owned(),
            },
        }
    }

    fn policy() -> ContextPolicy {
        ContextPolicy {
            hard_input_tokens: 6_000,
        }
    }

    fn input<'a>(
        transcript: &'a CanonicalTranscript,
        projection: Option<&'a ContextProjection>,
        contract: &'a TaskContract,
        workspace: &'a WorkspaceState,
        receipts: &'a [EvidenceReceipt],
        rejection: Option<&'a CompletionRejection>,
    ) -> ContextInput<'a> {
        ContextInput {
            transcript,
            projection,
            task_contract: Some(contract),
            workspace_state: workspace,
            evidence_receipts: receipts,
            last_completion_rejection: rejection,
            last_verifier_failure: None,
            last_verifier_failure_workspace: None,
            tools: &[],
        }
    }

    fn long_transcript(contract: &TaskContract) -> CanonicalTranscript {
        let mut entries = vec![
            TranscriptEntry::System {
                prompt: SystemPrompt::from_text("stable system"),
            },
            TranscriptEntry::User {
                content: contract.definition.model_message(),
            },
        ];
        for index in 0..8 {
            entries.push(TranscriptEntry::Assistant {
                content: None,
                reasoning_content: Some(format!("reasoning-{index}")),
                tool_calls: vec![ModelToolCall {
                    id: format!("call-{index}"),
                    name: "read_file".to_owned(),
                    arguments: ToolArguments::from_value(json!({"path": format!("{index}.rs")})),
                }],
            });
            entries.push(TranscriptEntry::Tool {
                call_id: format!("call-{index}"),
                name: "read_file".to_owned(),
                outcome: Box::new(ToolOutcome::success("甲".repeat(4_000))),
            });
        }
        entries.push(TranscriptEntry::User {
            content: "第五条之后仍有效的约束".to_owned(),
        });
        CanonicalTranscript { entries }
    }

    #[test]
    fn task_contract_and_current_host_facts_survive_repeated_compaction_once() {
        let contract = contract();
        let workspace = workspace(3, "sha256:current");
        let transcript = long_transcript(&contract);
        let canonical = transcript.clone();

        let ContextCompactionPreparation::Local {
            projection: first, ..
        } = prepare_compaction(
            input(&transcript, None, &contract, &workspace, &[], None),
            policy(),
        )
        .expect("first plan")
        else {
            panic!("expected local compaction");
        };
        let projected = effective_context(input(
            &transcript,
            Some(&first),
            &contract,
            &workspace,
            &[],
            None,
        ))
        .expect("first projection");
        let rendered = serde_json::to_string(&projected.messages).expect("messages");
        assert_eq!(rendered.matches("SENTINEL_CONSTRAINT").count(), 1);
        assert_eq!(rendered.matches("第五条之后仍有效的约束").count(), 1);
        assert_eq!(transcript, canonical);

        let mut extended = transcript.clone();
        extended.entries.push(TranscriptEntry::User {
            content: "最新约束".to_owned(),
        });
        let second = prepare_compaction(
            input(&extended, Some(&first), &contract, &workspace, &[], None),
            policy(),
        )
        .expect("second plan");
        let second_projection = match &second {
            ContextCompactionPreparation::Local { projection, .. } => projection,
            ContextCompactionPreparation::NotNeeded { .. } => &first,
            ContextCompactionPreparation::LimitExceeded { .. } => {
                panic!("repeated compaction must remain within the hard limit")
            }
        };
        let projected = effective_context(input(
            &extended,
            Some(second_projection),
            &contract,
            &workspace,
            &[],
            None,
        ))
        .expect("second projection");
        let rendered = serde_json::to_string(&projected.messages).expect("messages");
        assert_eq!(rendered.matches("SENTINEL_CONSTRAINT").count(), 1);
        assert_eq!(rendered.matches("最新约束").count(), 1);
    }

    #[test]
    fn receipt_requires_exact_workspace_generation_not_only_revision_hash() {
        let contract = contract();
        let transcript = long_transcript(&contract);
        let old_workspace = workspace(4, "sha256:same");
        let receipt = EvidenceReceipt {
            id: EvidenceReceiptId::from("receipt-1"),
            generation_id: contract.generation_id.clone(),
            acceptance_id: AcceptanceId::from("accept-1"),
            verification_id: VerificationId::from("verification-1"),
            verifier: verifier(),
            workspace_state: old_workspace.clone(),
            artifact_ids: vec!["artifact-1".to_owned()],
        };
        let receipts = vec![receipt];
        let current_workspace = workspace(5, "sha256:same");
        let context = effective_context(input(
            &transcript,
            None,
            &contract,
            &current_workspace,
            &receipts,
            None,
        ))
        .expect("context");
        let rendered = serde_json::to_string(&context.messages).expect("messages");
        assert!(!rendered.contains("receipt-1"));
        assert!(rendered.contains("current_evidence_receipt"));
        assert!(rendered.contains("`none`"));
    }

    #[test]
    fn joined_child_handoff_and_reasoning_tool_group_remain_atomic() {
        let contract = contract();
        let workspace = workspace(1, "sha256:workspace");
        let mut transcript = long_transcript(&contract);
        transcript.entries.push(TranscriptEntry::ChildOutcome {
            call_id: "agent-call".to_owned(),
            child_run_id: RunId::from("child-1"),
            outcome: Box::new(AgentOutcome {
                run_id: RunId::from("child-1"),
                parent_run_id: Some(RunId::from("run-1")),
                terminal: codewhale_protocol::agent_runtime::TerminalState::Blocked {
                    reason: "done".to_owned(),
                },
                accounting: ModelAccounting::default(),
                runtime_model_requests: 1,
                runtime_retries: 0,
                tool_calls: 1,
                details: Default::default(),
            }),
            handoff_content: "CHILD_HANDOFF_SENTINEL".to_owned(),
        });

        let ContextCompactionPreparation::Local { projection, .. } = prepare_compaction(
            input(&transcript, None, &contract, &workspace, &[], None),
            policy(),
        )
        .expect("plan") else {
            panic!("expected local compaction");
        };
        let context = effective_context(input(
            &transcript,
            Some(&projection),
            &contract,
            &workspace,
            &[],
            None,
        ))
        .expect("context");
        assert!(context.messages.iter().any(
            |message| matches!(message, ModelMessage::User { content } if content == "CHILD_HANDOFF_SENTINEL")
        ));
        for (index, message) in context.messages.iter().enumerate() {
            let ModelMessage::Assistant { tool_calls, .. } = message else {
                continue;
            };
            for call in tool_calls {
                assert!(context.messages[index + 1..].iter().any(|candidate| {
                    matches!(candidate, ModelMessage::Tool { call_id, .. } if call_id == &call.id)
                }));
            }
        }
    }

    #[test]
    fn every_current_task_mutation_group_survives_compaction() {
        let contract = contract();
        let workspace = workspace(3, "sha256:after-two-writes");
        let mut transcript = long_transcript(&contract);
        for (call_id, path, sentinel, revision) in [
            (
                "write-first",
                "src/first.rs",
                "FIRST_MUTATION_SENTINEL",
                "sha256:first",
            ),
            (
                "write-second",
                "src/second.rs",
                "SECOND_MUTATION_SENTINEL",
                "sha256:second",
            ),
        ] {
            transcript.entries.push(TranscriptEntry::Assistant {
                content: None,
                reasoning_content: Some(format!("修改 {path}")),
                tool_calls: vec![ModelToolCall {
                    id: call_id.to_owned(),
                    name: "apply_patch".to_owned(),
                    arguments: ToolArguments::from_value(json!({"path": path})),
                }],
            });
            let mut outcome = ToolOutcome::success(sentinel);
            outcome.side_effect = ToolSideEffectStatus::Applied;
            outcome.workspace_revision = Some(revision.to_owned());
            transcript.entries.push(TranscriptEntry::Tool {
                call_id: call_id.to_owned(),
                name: "apply_patch".to_owned(),
                outcome: Box::new(outcome),
            });
        }

        let ContextCompactionPreparation::Local { projection, .. } = prepare_compaction(
            input(&transcript, None, &contract, &workspace, &[], None),
            policy(),
        )
        .expect("plan") else {
            panic!("expected local compaction");
        };
        let context = effective_context(input(
            &transcript,
            Some(&projection),
            &contract,
            &workspace,
            &[],
            None,
        ))
        .expect("context");
        let rendered = serde_json::to_string(&context.messages).expect("messages");
        for sentinel in ["FIRST_MUTATION_SENTINEL", "SECOND_MUTATION_SENTINEL"] {
            assert_eq!(rendered.matches(sentinel).count(), 1);
        }
        for call_id in ["write-first", "write-second"] {
            assert_eq!(rendered.matches(call_id).count(), 2);
        }
    }

    #[test]
    fn mandatory_facts_over_hard_limit_fail_closed_without_summary() {
        let mut contract = contract();
        contract.definition.constraints = vec!["约束".repeat(20_000)];
        let workspace = workspace(1, "sha256:workspace");
        let transcript = CanonicalTranscript {
            entries: vec![
                TranscriptEntry::System {
                    prompt: SystemPrompt::from_text("system"),
                },
                TranscriptEntry::User {
                    content: contract.definition.model_message(),
                },
            ],
        };
        let outcome = prepare_compaction(
            input(&transcript, None, &contract, &workspace, &[], None),
            ContextPolicy {
                hard_input_tokens: 1_000,
            },
        )
        .expect("plan");
        assert!(matches!(
            outcome,
            ContextCompactionPreparation::LimitExceeded { .. }
        ));
    }
}
