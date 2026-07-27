//! Shared human projection for canonical model-request failures.
//!
//! This module formats stored Runtime facts only. It owns no retry decision,
//! transport behavior, persistence, or terminal state.

use dse_localization::{MessageId, ProductLanguage, tr_in};
use dse_protocol::agent_runtime::{
    ModelAttemptFailure, ModelErrorCategory, ModelRetryDecision, ModelRetryStopReason,
};

pub(crate) fn model_request_failed_message(
    language: ProductLanguage,
    failure: &ModelAttemptFailure,
    retry: &ModelRetryDecision,
) -> String {
    let mut message = tr_in(language, MessageId::RunModelRequestFailed)
        .replace("{failure}", &model_failure_label(language, failure))
        .replace("{retry}", &model_retry_label(language, retry));
    if matches!(retry, ModelRetryDecision::Stop { .. }) {
        message.push_str(&tr_in(language, MessageId::RunRetryStoppedNextAction));
    }
    message
}

pub(crate) fn model_request_status_message(
    language: ProductLanguage,
    failure: &ModelAttemptFailure,
    retry: &ModelRetryDecision,
) -> String {
    if let ModelRetryDecision::Retry { prepared } = retry {
        return tr_in(language, MessageId::RunModelRetryStatus)
            .replace(
                "{category}",
                &model_error_category_label(language, failure.category),
            )
            .replace("{attempt}", &prepared.request.attempt.to_string())
            .replace("{max_retries}", &prepared.max_retries.to_string())
            .replace(
                "{seconds}",
                &prepared.backoff_ms.div_ceil(1_000).to_string(),
            );
    }
    model_request_failed_message(language, failure, retry)
}

fn model_failure_label(language: ProductLanguage, failure: &ModelAttemptFailure) -> String {
    tr_in(language, MessageId::RunModelFailureDetail)
        .replace("{message}", &failure.message)
        .replace("{code}", &failure.code)
        .replace(
            "{category}",
            &model_error_category_label(language, failure.category),
        )
}

fn model_error_category_label(
    language: ProductLanguage,
    category: ModelErrorCategory,
) -> std::borrow::Cow<'static, str> {
    match category {
        ModelErrorCategory::Transport => tr_in(language, MessageId::RunModelCategoryTransport),
        ModelErrorCategory::Timeout => tr_in(language, MessageId::RunModelCategoryTimeout),
        ModelErrorCategory::StreamStall => tr_in(language, MessageId::RunModelCategoryStreamStall),
        ModelErrorCategory::RateLimit => tr_in(language, MessageId::RunModelCategoryRateLimit),
        ModelErrorCategory::Authentication => {
            tr_in(language, MessageId::RunModelCategoryAuthentication)
        }
        ModelErrorCategory::Protocol => tr_in(language, MessageId::RunModelCategoryProtocol),
        ModelErrorCategory::Service => tr_in(language, MessageId::RunModelCategoryService),
        ModelErrorCategory::Cancelled => tr_in(language, MessageId::RunModelCategoryCancelled),
        ModelErrorCategory::Unknown => tr_in(language, MessageId::RunModelCategoryUnknown),
    }
}

pub(crate) fn model_retry_label(language: ProductLanguage, retry: &ModelRetryDecision) -> String {
    match retry {
        ModelRetryDecision::Stop { reason } => tr_in(language, MessageId::RunRetryStopped).replace(
            "{reason}",
            &model_retry_stop_reason_label(language, *reason),
        ),
        ModelRetryDecision::Retry { prepared } => tr_in(language, MessageId::RunRetryPrepared)
            .replace("{attempt}", &prepared.request.attempt.to_string())
            .replace("{max_retries}", &prepared.max_retries.to_string())
            .replace(
                "{seconds}",
                &prepared.backoff_ms.div_ceil(1_000).to_string(),
            ),
    }
}

fn model_retry_stop_reason_label(
    language: ProductLanguage,
    reason: ModelRetryStopReason,
) -> std::borrow::Cow<'static, str> {
    match reason {
        ModelRetryStopReason::ActionableOutput => {
            tr_in(language, MessageId::RunRetryActionableOutput)
        }
        ModelRetryStopReason::UnsafeReplay => tr_in(language, MessageId::RunRetryUnsafeReplay),
        ModelRetryStopReason::NotRetryable => tr_in(language, MessageId::RunRetryNotRetryable),
        ModelRetryStopReason::FailureChanged => tr_in(language, MessageId::RunRetryFailureChanged),
        ModelRetryStopReason::RetryLimitReached => tr_in(language, MessageId::RunRetryLimitReached),
        ModelRetryStopReason::ModelRequestBudgetExceeded => {
            tr_in(language, MessageId::RunRetryBudgetExceeded)
        }
    }
}
