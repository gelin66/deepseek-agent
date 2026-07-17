//! Deterministic planning for canonical context compaction.
//!
//! The full `CanonicalTranscript` is never rewritten. This module only
//! materializes and compacts the model-visible request projection.

use codewhale_protocol::agent_runtime::{
    CanonicalTranscript, ContextCompactionPlan, ContextPolicy, ContextProjection, ModelMessage,
    ModelRequest, PromptCacheControl, ReasoningEffort, SystemPrompt, SystemPromptBlock,
    TranscriptEntry,
};
use sha2::{Digest, Sha256};

const TOOL_RESULT_PRUNE_CHARS: usize = 16 * 1024;
const TOOL_RESULT_RETAIN_CHARS: usize = 2 * 1024;
const FALLBACK_SUMMARY_MAX_CHARS: usize = 120_000;
const FALLBACK_SUMMARY_HEAD_CHARS: usize = 72_000;
const FALLBACK_SUMMARY_TAIL_CHARS: usize = 36_000;

const SUMMARY_INSTRUCTION: &str = "\
请把此前的编码任务上下文压缩成一份可继续执行的中文工作摘要。必须保留：用户目标和最新约束、\
已做决定、修改过的文件与关键代码事实、尚未解决的问题、工具调用的重要结果、测试与验证证据、\
下一步动作。不要声称未验证的结果，不要输出寒暄，只输出结构清晰的摘要。";

#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveContext {
    pub system_prompt: SystemPrompt,
    pub messages: Vec<ModelMessage>,
    pub user_message_indices: Vec<u32>,
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
    Model {
        plan: ContextCompactionPlan,
        request: ModelRequest,
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
    #[error("failed to serialize context projection for a stable digest: {0}")]
    Digest(String),
    #[error("invalid context policy: {0}")]
    InvalidPolicy(String),
    #[error("failed to serialize bounded context-summary input: {0}")]
    SummaryInput(String),
    #[error("invalid context projection user-message index metadata")]
    InvalidUserMessageIndices,
}

pub fn effective_context(
    transcript: &CanonicalTranscript,
    projection: Option<&ContextProjection>,
) -> Result<EffectiveContext, ContextProjectionError> {
    let base_system_prompt = transcript
        .entries
        .iter()
        .find_map(|entry| match entry {
            TranscriptEntry::System { prompt } => Some(prompt.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let (system_prompt, messages, user_message_indices, source_entry_count) = match projection {
        Some(projection) => {
            let boundary = usize::try_from(projection.source_entry_count).unwrap_or(usize::MAX);
            if boundary > transcript.entries.len() {
                return Err(ContextProjectionError::BoundaryAhead {
                    boundary: projection.source_entry_count,
                    entries: transcript.entries.len(),
                });
            }
            validate_user_message_indices(&projection.messages, &projection.user_message_indices)?;
            let mut messages = projection.messages.clone();
            let mut user_message_indices = projection.user_message_indices.clone();
            let (suffix, suffix_user_indices) = project_entries(&transcript.entries[boundary..]);
            let offset = u32::try_from(messages.len()).unwrap_or(u32::MAX);
            messages.extend(suffix);
            user_message_indices.extend(
                suffix_user_indices
                    .into_iter()
                    .map(|index| index.saturating_add(offset)),
            );
            (
                merge_summary_prompt(&base_system_prompt, projection.summary_prompt.as_ref()),
                messages,
                user_message_indices,
                u64::try_from(transcript.entries.len()).unwrap_or(u64::MAX),
            )
        }
        None => {
            let (messages, user_message_indices) = project_entries(&transcript.entries);
            (
                base_system_prompt,
                messages,
                user_message_indices,
                u64::try_from(transcript.entries.len()).unwrap_or(u64::MAX),
            )
        }
    };
    let sha256 = projection_digest(
        &system_prompt,
        &messages,
        &user_message_indices,
        source_entry_count,
    )?;
    let estimated_tokens = estimate_context_tokens(&system_prompt, &messages);
    Ok(EffectiveContext {
        system_prompt,
        messages,
        user_message_indices,
        source_entry_count,
        sha256,
        estimated_tokens,
    })
}

pub fn prepare_compaction(
    transcript: &CanonicalTranscript,
    projection: Option<&ContextProjection>,
    policy: ContextPolicy,
    request_template: &ModelRequest,
    force: bool,
) -> Result<ContextCompactionPreparation, ContextProjectionError> {
    let effective = effective_context(transcript, projection)?;
    if !policy.auto_compact && !force {
        return Ok(ContextCompactionPreparation::NotNeeded {
            estimated_tokens: effective.estimated_tokens,
        });
    }
    validate_policy(policy)?;
    let trigger = u64::from(policy.trigger_tokens);
    if !force && effective.estimated_tokens <= trigger {
        return Ok(ContextCompactionPreparation::NotNeeded {
            estimated_tokens: effective.estimated_tokens,
        });
    }

    let cutoff = retained_suffix_start(
        &effective.user_message_indices,
        effective.messages.len(),
        usize::try_from(policy.keep_recent_user_turns).unwrap_or(usize::MAX),
    );
    let mut locally_pruned = effective.messages.clone();
    let pruned = prune_old_tool_results(&mut locally_pruned[..cutoff]);
    let locally_pruned_tokens = estimate_context_tokens(&effective.system_prompt, &locally_pruned);
    let existing_summary_prompt = projection.and_then(|value| value.summary_prompt.clone());
    if pruned && !force && locally_pruned_tokens <= u64::from(policy.trigger_tokens) {
        return Ok(ContextCompactionPreparation::Local {
            projection: ContextProjection {
                source_entry_count: effective.source_entry_count,
                source_projection_sha256: effective.sha256,
                summary_prompt: existing_summary_prompt,
                messages: locally_pruned,
                user_message_indices: effective.user_message_indices,
            },
            before_tokens: effective.estimated_tokens,
            after_tokens: locally_pruned_tokens,
        });
    }

    let min_messages = usize::try_from(policy.min_messages).unwrap_or(usize::MAX);
    if cutoff < min_messages {
        if pruned && locally_pruned_tokens <= u64::from(policy.hard_input_tokens) {
            return Ok(ContextCompactionPreparation::Local {
                projection: ContextProjection {
                    source_entry_count: effective.source_entry_count,
                    source_projection_sha256: effective.sha256,
                    summary_prompt: existing_summary_prompt,
                    messages: locally_pruned,
                    user_message_indices: effective.user_message_indices,
                },
                before_tokens: effective.estimated_tokens,
                after_tokens: locally_pruned_tokens,
            });
        }
        if locally_pruned_tokens > u64::from(policy.hard_input_tokens) {
            return Ok(ContextCompactionPreparation::LimitExceeded {
                estimated_tokens: locally_pruned_tokens,
                hard_input_tokens: u64::from(policy.hard_input_tokens),
            });
        }
        return Ok(ContextCompactionPreparation::NotNeeded {
            estimated_tokens: effective.estimated_tokens,
        });
    }

    let summary_source = locally_pruned[..cutoff].to_vec();
    let retained_messages = locally_pruned[cutoff..].to_vec();
    let cutoff_u32 = u32::try_from(cutoff).unwrap_or(u32::MAX);
    let retained_user_message_indices = effective
        .user_message_indices
        .iter()
        .copied()
        .filter(|index| *index >= cutoff_u32)
        .map(|index| index.saturating_sub(cutoff_u32))
        .collect::<Vec<_>>();
    let mut request = request_template.clone();
    request.system_prompt = effective.system_prompt.clone();
    request.messages = summary_source.clone();
    request.messages.push(ModelMessage::User {
        content: SUMMARY_INSTRUCTION.to_owned(),
    });
    request.tools.clear();
    request.reasoning_effort = ReasoningEffort::Low;
    request.max_output_tokens = Some(policy.summary_max_output_tokens.max(1));
    request.streaming = false;
    request.attempt = 0;

    let mut bounded_request = request.clone();
    bounded_request.system_prompt = SystemPrompt::from_text(
        "你是编码 Agent 的上下文压缩器。准确保留事实、约束、工作状态和验证证据。",
    );
    bounded_request.messages = vec![ModelMessage::User {
        content: bounded_summary_input(&summary_source)?,
    }];
    if estimate_context_tokens(&request.system_prompt, &request.messages)
        > u64::from(policy.hard_input_tokens)
    {
        request = bounded_request;
    }

    Ok(ContextCompactionPreparation::Model {
        plan: ContextCompactionPlan {
            source_entry_count: effective.source_entry_count,
            source_projection_sha256: effective.sha256,
            before_tokens: effective.estimated_tokens,
            locally_pruned_tokens,
            retained_messages,
            retained_user_message_indices,
        },
        request,
    })
}

pub fn projection_from_summary(plan: &ContextCompactionPlan, summary: &str) -> ContextProjection {
    ContextProjection {
        source_entry_count: plan.source_entry_count,
        source_projection_sha256: plan.source_projection_sha256.clone(),
        summary_prompt: Some(SystemPrompt {
            blocks: vec![SystemPromptBlock {
                text: format!(
                    "以下是此前编码工作的压缩摘要。它是上下文投影，不替代完整运行记录：\n\n{}",
                    summary.trim()
                ),
                cache_control: PromptCacheControl::Volatile,
            }],
        }),
        messages: plan.retained_messages.clone(),
        user_message_indices: plan.retained_user_message_indices.clone(),
    }
}

pub fn estimate_projection_tokens(
    transcript: &CanonicalTranscript,
    projection: &ContextProjection,
) -> Result<u64, ContextProjectionError> {
    Ok(effective_context(transcript, Some(projection))?.estimated_tokens)
}

fn project_entries(entries: &[TranscriptEntry]) -> (Vec<ModelMessage>, Vec<u32>) {
    let mut messages = Vec::new();
    let mut user_message_indices = Vec::new();
    for entry in entries {
        if matches!(entry, TranscriptEntry::System { .. }) {
            continue;
        }
        if matches!(entry, TranscriptEntry::User { .. }) {
            user_message_indices.push(u32::try_from(messages.len()).unwrap_or(u32::MAX));
        }
        let mut projected = CanonicalTranscript {
            entries: vec![entry.clone()],
        }
        .project_messages();
        messages.append(&mut projected);
    }
    (messages, user_message_indices)
}

fn merge_summary_prompt(base: &SystemPrompt, summary: Option<&SystemPrompt>) -> SystemPrompt {
    let mut merged = base.clone();
    if let Some(summary) = summary {
        merged.blocks.extend(summary.blocks.clone());
    }
    merged
}

fn retained_suffix_start(
    user_message_indices: &[u32],
    message_count: usize,
    keep_user_turns: usize,
) -> usize {
    if keep_user_turns == 0 {
        return message_count;
    }
    if user_message_indices.len() <= keep_user_turns {
        return 0;
    }
    usize::try_from(user_message_indices[user_message_indices.len() - keep_user_turns])
        .unwrap_or(message_count)
        .min(message_count)
}

fn validate_user_message_indices(
    messages: &[ModelMessage],
    indices: &[u32],
) -> Result<(), ContextProjectionError> {
    let valid = indices.windows(2).all(|pair| pair[0] < pair[1])
        && indices.iter().all(|index| {
            messages
                .get(usize::try_from(*index).unwrap_or(usize::MAX))
                .is_some_and(|message| matches!(message, ModelMessage::User { .. }))
        });
    if valid {
        Ok(())
    } else {
        Err(ContextProjectionError::InvalidUserMessageIndices)
    }
}

fn prune_old_tool_results(messages: &mut [ModelMessage]) -> bool {
    let mut changed = false;
    for message in messages {
        let ModelMessage::Tool { content, name, .. } = message else {
            continue;
        };
        if content.chars().count() <= TOOL_RESULT_PRUNE_CHARS {
            continue;
        }
        let original = content.chars().count();
        let head = take_chars(content, TOOL_RESULT_RETAIN_CHARS);
        let tail = take_tail_chars(content, TOOL_RESULT_RETAIN_CHARS);
        *content = format!(
            "[{name} 的旧工具结果已在请求投影中压缩，原始记录保留 {original} 字符]\n{head}\n…\n{tail}"
        );
        changed = true;
    }
    changed
}

fn validate_policy(policy: ContextPolicy) -> Result<(), ContextProjectionError> {
    if policy.context_window_tokens == 0 {
        return Err(ContextProjectionError::InvalidPolicy(
            "context_window_tokens must be non-zero".to_owned(),
        ));
    }
    if policy.trigger_tokens == 0
        || policy.hard_input_tokens == 0
        || policy.trigger_tokens > policy.hard_input_tokens
        || policy.hard_input_tokens >= policy.context_window_tokens
    {
        return Err(ContextProjectionError::InvalidPolicy(
            "expected 0 < trigger_tokens <= hard_input_tokens < context_window_tokens".to_owned(),
        ));
    }
    if policy.summary_max_output_tokens == 0 {
        return Err(ContextProjectionError::InvalidPolicy(
            "summary_max_output_tokens must be non-zero".to_owned(),
        ));
    }
    if policy
        .hard_input_tokens
        .saturating_add(policy.summary_max_output_tokens)
        > policy.context_window_tokens
    {
        return Err(ContextProjectionError::InvalidPolicy(
            "hard input plus summary output reservation exceeds the context window".to_owned(),
        ));
    }
    Ok(())
}

fn bounded_summary_input(messages: &[ModelMessage]) -> Result<String, ContextProjectionError> {
    let rendered = serde_json::to_string(messages)
        .map_err(|error| ContextProjectionError::SummaryInput(error.to_string()))?;
    let body = if rendered.chars().count() <= FALLBACK_SUMMARY_MAX_CHARS {
        rendered
    } else {
        format!(
            "{}\n…[中间内容省略，仅用于本次摘要请求]…\n{}",
            take_chars(&rendered, FALLBACK_SUMMARY_HEAD_CHARS),
            take_tail_chars(&rendered, FALLBACK_SUMMARY_TAIL_CHARS)
        )
    };
    Ok(format!(
        "{SUMMARY_INSTRUCTION}\n\n需要压缩的历史 JSON：\n{body}"
    ))
}

fn projection_digest(
    system_prompt: &SystemPrompt,
    messages: &[ModelMessage],
    user_message_indices: &[u32],
    source_entry_count: u64,
) -> Result<String, ContextProjectionError> {
    let bytes = serde_json::to_vec(&(
        system_prompt,
        messages,
        user_message_indices,
        source_entry_count,
    ))
    .map_err(|error| ContextProjectionError::Digest(error.to_string()))?;
    let digest = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("sha256:{digest}"))
}

fn estimate_context_tokens(system_prompt: &SystemPrompt, messages: &[ModelMessage]) -> u64 {
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
                .saturating_add(if tool_calls.is_empty() {
                    0
                } else {
                    reasoning_content.as_deref().map_or(0, estimate_text_tokens)
                })
                .saturating_add(
                    tool_calls
                        .iter()
                        .map(|call| {
                            estimate_text_tokens(&call.name)
                                .saturating_add(estimate_text_tokens(&call.arguments.raw))
                        })
                        .sum::<usize>(),
                ),
            ModelMessage::Tool { name, content, .. } => {
                estimate_text_tokens(name).saturating_add(estimate_text_tokens(content))
            }
        })
        .sum::<usize>();
    let lexical_tokens = system_tokens.saturating_add(message_tokens);
    u64::try_from(lexical_tokens)
        .unwrap_or(u64::MAX)
        .saturating_add(
            u64::try_from(messages.len())
                .unwrap_or(u64::MAX)
                .saturating_mul(12),
        )
        .saturating_add(48)
}

/// Conservative tokenizer-free estimate calibrated against the official
/// DeepSeek usage response for Chinese, repetitive ASCII, and high-entropy
/// hexadecimal input.
///
/// Ordinary words retain useful context capacity. Dense mixed alphanumeric
/// data, digits, punctuation, and four-byte Unicode use stricter bounds because
/// they tokenize much less efficiently than prose.
fn estimate_text_tokens(value: &str) -> usize {
    let bytes = value.as_bytes();
    let mut tokens = 0usize;
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if !bytes[cursor].is_ascii() {
            let character = value[cursor..]
                .chars()
                .next()
                .expect("cursor is always on a UTF-8 character boundary");
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
        AgentActor, AgentOutcome, ContextPolicy, ModelAccounting, ModelMessage, RunId,
        TerminalState, ToolArguments,
    };
    use serde_json::json;

    use super::*;

    fn request(messages: Vec<ModelMessage>) -> ModelRequest {
        ModelRequest {
            run_id: RunId::from("run-1"),
            parent_run_id: None,
            actor: AgentActor::default(),
            model: "deepseek-v4-pro".to_owned(),
            system_prompt: SystemPrompt::from_text("system"),
            messages,
            tools: Vec::new(),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: Some(8_192),
            streaming: true,
            request_number: 1,
            attempt: 0,
        }
    }

    fn transcript(turns: usize, tool_chars: usize) -> CanonicalTranscript {
        let mut entries = vec![TranscriptEntry::System {
            prompt: SystemPrompt::from_text("system"),
        }];
        for index in 0..turns {
            entries.push(TranscriptEntry::User {
                content: format!("用户任务 {index} {}", "甲".repeat(3_000)),
            });
            entries.push(TranscriptEntry::Assistant {
                content: None,
                reasoning_content: Some("推理".repeat(100)),
                tool_calls: vec![codewhale_protocol::agent_runtime::ModelToolCall {
                    id: format!("call-{index}"),
                    name: "read_file".to_owned(),
                    arguments: ToolArguments::from_value(
                        json!({"path": format!("src/{index}.rs")}),
                    ),
                }],
            });
            entries.push(TranscriptEntry::Tool {
                call_id: format!("call-{index}"),
                name: "read_file".to_owned(),
                outcome: codewhale_protocol::agent_runtime::ToolOutcome::success(
                    "结果".repeat(tool_chars / 2),
                ),
            });
        }
        CanonicalTranscript { entries }
    }

    fn policy() -> ContextPolicy {
        ContextPolicy {
            auto_compact: true,
            trigger_tokens: 1,
            hard_input_tokens: 500_000,
            context_window_tokens: 1_000_000,
            summary_max_output_tokens: 2_048,
            min_messages: 6,
            keep_recent_user_turns: 4,
            max_retries: 3,
        }
    }

    #[test]
    fn projection_keeps_canonical_transcript_and_tool_pairs_intact() {
        let transcript = transcript(8, 40_000);
        let effective = effective_context(&transcript, None).expect("effective context");
        let mut template = request(effective.messages.clone());
        template.system_prompt = effective.system_prompt;
        let preparation =
            prepare_compaction(&transcript, None, policy(), &template, false).expect("plan");
        let ContextCompactionPreparation::Model { plan, .. } = preparation else {
            panic!("expected model compaction")
        };
        assert_eq!(transcript.entries.len(), 25);
        assert!(plan.retained_messages.len() >= 12);
        for message in &plan.retained_messages {
            if let ModelMessage::Assistant { tool_calls, .. } = message {
                for call in tool_calls {
                    assert!(plan.retained_messages.iter().any(|candidate| {
                        matches!(
                            candidate,
                            ModelMessage::Tool { call_id, .. } if call_id == &call.id
                        )
                    }));
                }
            }
        }
    }

    #[test]
    fn committed_projection_appends_only_new_canonical_suffix() {
        let mut transcript = transcript(6, 100);
        let effective = effective_context(&transcript, None).expect("effective");
        let template = request(effective.messages.clone());
        let ContextCompactionPreparation::Model { plan, .. } = prepare_compaction(
            &transcript,
            None,
            ContextPolicy {
                keep_recent_user_turns: 2,
                ..policy()
            },
            &template,
            true,
        )
        .expect("plan") else {
            panic!("expected model plan")
        };
        let projection = projection_from_summary(&plan, "已完成前半部分");
        transcript.entries.push(TranscriptEntry::User {
            content: "新的约束".to_owned(),
        });
        let projected =
            effective_context(&transcript, Some(&projection)).expect("projected context");
        assert!(matches!(
            projected.messages.last(),
            Some(ModelMessage::User { content }) if content == "新的约束"
        ));
        assert_eq!(
            projection.source_entry_count,
            u64::try_from(transcript.entries.len() - 1).unwrap()
        );
    }

    #[test]
    fn child_handoffs_do_not_consume_recent_user_turn_budget() {
        let mut entries = vec![TranscriptEntry::System {
            prompt: SystemPrompt::from_text("system"),
        }];
        for index in 0..6 {
            entries.push(TranscriptEntry::User {
                content: format!("真实用户约束 {index}"),
            });
            entries.push(TranscriptEntry::ChildOutcome {
                call_id: format!("child-{index}"),
                child_run_id: RunId::from(format!("child-run-{index}")),
                outcome: Box::new(AgentOutcome {
                    run_id: RunId::from(format!("child-run-{index}")),
                    parent_run_id: Some(RunId::from("run-1")),
                    terminal: TerminalState::Completed {
                        message: "完成".to_owned(),
                    },
                    accounting: ModelAccounting::default(),
                    runtime_model_requests: 0,
                    runtime_retries: 0,
                    tool_calls: 0,
                }),
                handoff_content: format!("子 Agent handoff {index}"),
            });
        }
        let transcript = CanonicalTranscript { entries };
        let effective = effective_context(&transcript, None).expect("effective");
        let ContextCompactionPreparation::Model { plan, .. } = prepare_compaction(
            &transcript,
            None,
            ContextPolicy {
                keep_recent_user_turns: 2,
                min_messages: 2,
                ..policy()
            },
            &request(effective.messages),
            true,
        )
        .expect("plan") else {
            panic!("expected model compaction")
        };
        assert_eq!(plan.retained_user_message_indices.len(), 2);
        assert!(plan.retained_messages.iter().any(
            |message| matches!(message, ModelMessage::User { content } if content == "真实用户约束 4")
        ));
        assert!(plan.retained_messages.iter().any(
            |message| matches!(message, ModelMessage::User { content } if content == "真实用户约束 5")
        ));
    }

    #[test]
    fn repeated_compaction_replaces_instead_of_stacking_summary() {
        let mut transcript = transcript(8, 100);
        let first_effective = effective_context(&transcript, None).expect("effective");
        let ContextCompactionPreparation::Model {
            plan: first_plan, ..
        } = prepare_compaction(
            &transcript,
            None,
            ContextPolicy {
                keep_recent_user_turns: 2,
                ..policy()
            },
            &request(first_effective.messages),
            true,
        )
        .expect("first plan")
        else {
            panic!("expected first model compaction")
        };
        let first = projection_from_summary(&first_plan, "旧摘要标记");
        for index in 0..4 {
            transcript.entries.push(TranscriptEntry::User {
                content: format!("后续约束 {index}"),
            });
            transcript.entries.push(TranscriptEntry::Assistant {
                content: Some(format!("后续答复 {index}")),
                reasoning_content: None,
                tool_calls: Vec::new(),
            });
        }
        let second_effective =
            effective_context(&transcript, Some(&first)).expect("second effective");
        let ContextCompactionPreparation::Model {
            plan: second_plan,
            request: second_request,
        } = prepare_compaction(
            &transcript,
            Some(&first),
            ContextPolicy {
                keep_recent_user_turns: 2,
                ..policy()
            },
            &request(second_effective.messages),
            true,
        )
        .expect("second plan")
        else {
            panic!("expected second model compaction")
        };
        assert!(
            second_request
                .system_prompt
                .blocks
                .iter()
                .any(|block| block.text.contains("旧摘要标记"))
        );
        let second = projection_from_summary(&second_plan, "新摘要标记");
        let projected = effective_context(&transcript, Some(&second)).expect("second projection");
        let prompt = projected
            .system_prompt
            .blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(prompt.contains("新摘要标记"));
        assert!(!prompt.contains("旧摘要标记"));
    }

    #[test]
    fn oversized_primary_uses_bounded_summary_request_before_transport() {
        let mut entries = vec![TranscriptEntry::System {
            prompt: SystemPrompt::from_text("system"),
        }];
        for index in 0..10 {
            entries.push(TranscriptEntry::User {
                content: format!("用户约束 {index} {}", "甲".repeat(60_000)),
            });
            entries.push(TranscriptEntry::Assistant {
                content: Some("已记录".to_owned()),
                reasoning_content: None,
                tool_calls: Vec::new(),
            });
        }
        let transcript = CanonicalTranscript { entries };
        let effective = effective_context(&transcript, None).expect("effective");
        let ContextCompactionPreparation::Model { request, .. } = prepare_compaction(
            &transcript,
            None,
            ContextPolicy {
                context_window_tokens: 200_000,
                trigger_tokens: 50_000,
                hard_input_tokens: 100_000,
                summary_max_output_tokens: 2_048,
                keep_recent_user_turns: 1,
                ..policy()
            },
            &request(effective.messages),
            true,
        )
        .expect("bounded plan") else {
            panic!("expected model compaction")
        };
        assert_eq!(request.messages.len(), 1);
        assert!(matches!(
            &request.messages[0],
            ModelMessage::User { content }
                if content.contains("需要压缩的历史 JSON")
                    && content.chars().count() < 130_000
        ));
    }

    #[test]
    fn token_estimate_covers_live_calibrated_chinese_and_dense_ascii() {
        let chinese = vec![ModelMessage::User {
            content: "甲".repeat(3_000),
        }];
        let ascii = vec![ModelMessage::User {
            content: "a".repeat(3_000),
        }];
        let repeated_digits = vec![ModelMessage::User {
            content: "1".repeat(3_000),
        }];
        let repeated_punctuation = vec![ModelMessage::User {
            content: "=".repeat(3_000),
        }];
        let mut high_entropy_hex =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".repeat(47);
        high_entropy_hex.truncate(3_000);
        let high_entropy_letters = high_entropy_hex
            .bytes()
            .map(|byte| match byte {
                b'0'..=b'9' => char::from(b'a' + (byte - b'0')),
                b'a'..=b'f' => char::from(b'k' + (byte - b'a')),
                _ => unreachable!("fixture contains only lowercase hexadecimal"),
            })
            .collect::<String>();
        let high_entropy_four_letters = high_entropy_hex
            .bytes()
            .map(|byte| {
                let value = match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => 10 + (byte - b'a'),
                    _ => unreachable!("fixture contains only lowercase hexadecimal"),
                };
                char::from(b'a' + (value % 4))
            })
            .collect::<String>();
        let high_entropy = vec![ModelMessage::User {
            content: high_entropy_hex,
        }];
        let high_entropy_alpha = vec![ModelMessage::User {
            content: high_entropy_letters,
        }];
        let high_entropy_four_alpha = vec![ModelMessage::User {
            content: high_entropy_four_letters,
        }];
        let prompt = SystemPrompt::default();
        assert!(
            estimate_context_tokens(&prompt, &chinese) >= 3_004,
            "Chinese estimate must cover the observed official usage"
        );
        assert!(
            estimate_context_tokens(&prompt, &ascii) >= 379,
            "repetitive ASCII estimate must cover the observed official usage"
        );
        assert!(
            estimate_context_tokens(&prompt, &ascii) < 1_000,
            "repetitive ASCII must retain substantially more capacity than high-entropy data"
        );
        assert!(
            estimate_context_tokens(&prompt, &repeated_digits) >= 1_500,
            "numeric runs must not use the alphabetic repeated-character discount"
        );
        assert!(
            estimate_context_tokens(&prompt, &repeated_punctuation) >= 1_500,
            "punctuation runs require a stricter bound than repetitive letters"
        );
        assert!(
            estimate_context_tokens(&prompt, &high_entropy) >= 1_708,
            "high-entropy hexadecimal estimate must cover the observed official usage"
        );
        assert!(
            estimate_context_tokens(&prompt, &high_entropy_alpha) >= 1_534,
            "high-entropy alphabetic estimate must cover the observed official usage"
        );
        assert!(
            estimate_context_tokens(&prompt, &high_entropy_four_alpha) >= 1_417,
            "dense four-letter estimate must cover the observed official usage"
        );
    }

    #[test]
    fn projection_digest_covers_user_origin_metadata() {
        let transcript = CanonicalTranscript {
            entries: vec![
                TranscriptEntry::System {
                    prompt: SystemPrompt::from_text("system"),
                },
                TranscriptEntry::User {
                    content: "用户一".to_owned(),
                },
                TranscriptEntry::User {
                    content: "用户二".to_owned(),
                },
            ],
        };
        let base = effective_context(&transcript, None).expect("base");
        let first = ContextProjection {
            source_entry_count: 3,
            source_projection_sha256: base.sha256.clone(),
            summary_prompt: None,
            messages: base.messages.clone(),
            user_message_indices: vec![0],
        };
        let second = ContextProjection {
            user_message_indices: vec![1],
            ..first.clone()
        };
        assert_ne!(
            effective_context(&transcript, Some(&first))
                .expect("first")
                .sha256,
            effective_context(&transcript, Some(&second))
                .expect("second")
                .sha256
        );
    }
}
