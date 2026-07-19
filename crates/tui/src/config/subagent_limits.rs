//! Sub-agent concurrency and DeepSeek stream-idle limits.
//!
//! The constants are re-exported through `config` for crate-wide use.

/// Temporary high-throughput default while the shared-context cutover makes
/// agent fanout cheap. This should eventually be governed by API/backpressure
/// budgets rather than memory-driven count throttles.
pub const DEFAULT_MAX_SUBAGENTS: usize = 64;
/// User-configurable ceiling for concurrent sub-agent execution. Keep this
/// above the default so operators can opt into larger API-bound fanout without
/// code changes while the full resource budget gate lands.
pub const MAX_SUBAGENTS: usize = 128;
/// Upper bound for queued + running sub-agent admissions. This is deliberately
/// higher than the instantaneous concurrency cap so bounded fanout can
/// opt into large bounded populations without unbounded queue growth.
pub const MAX_SUBAGENT_ADMISSION: usize = 1024;
/// Default per-SSE-chunk idle timeout, in seconds.
pub const DEFAULT_STREAM_CHUNK_TIMEOUT_SECS: u64 = 900;
/// Minimum accepted stream chunk timeout.
pub const MIN_STREAM_CHUNK_TIMEOUT_SECS: u64 = 1;
/// Maximum accepted stream chunk timeout.
pub const MAX_STREAM_CHUNK_TIMEOUT_SECS: u64 = 3600;
pub(crate) const STREAM_CHUNK_TIMEOUT_ENV: &str = "DEEPSEEK_STREAM_IDLE_TIMEOUT_SECS";
