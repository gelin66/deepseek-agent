//! Thin JSON-RPC over stdio client for LSP servers.
//!
//! We deliberately do **not** depend on `tower-lsp` — it is a server-side
//! framework and dragging it in here would add hundreds of unnecessary
//! transitive dependencies and slow down `cargo build` for every contributor.
//! The LSP wire protocol is small enough that handling it ourselves is a
//! self-contained ~400 LOC and lets us keep total control of the spawn
//! lifecycle, timeouts, and the async surface.
//!
//! Architecture:
//!
//! - [`LspTransport`] is the trait the [`super::LspManager`] talks to. The
//!   real implementation is [`StdioLspTransport`] (forks an LSP server with
//!   `tokio::process::Command`); tests use `super::tests::FakeTransport`.
//! - [`StdioLspTransport`] runs three tokio tasks: a reader, a writer, and
//!   the public API. Communication uses tokio mpsc channels.
//! - We parse `Content-Length`-framed JSON-RPC and route inbound messages
//!   either to a per-request response slot (for replies) or to the
//!   diagnostics queue (for `textDocument/publishDiagnostics` notifications).
//!
//! The transport is one-shot per file in MVP form: the manager spawns a
//! transport on demand for a language and reuses it. We do not implement
//! workspace sync beyond didOpen/didChange because the goal is "post-edit
//! diagnostics," not full IDE smartness.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::diagnostics::{Diagnostic, Severity};
use crate::tools::shell::{ProcessTreeOwner, configure_process_tree, shutdown_tokio_process_tree};
use crate::utils::spawn_supervised;

const LSP_SHUTDOWN_GRACE: Duration = Duration::from_millis(1_000);
const LSP_TASK_JOIN_GRACE: Duration = Duration::from_millis(250);

/// Trait the LSP manager talks to. A real LSP server speaks this via stdio;
/// tests use an in-process fake.
#[async_trait]
pub trait LspTransport: Send + Sync {
    /// Notify the server that a file was opened or its contents updated, then
    /// wait up to `wait` for a `publishDiagnostics` notification for that
    /// file. Returns the diagnostics list (possibly empty). Implementations
    /// must NOT block past `wait`.
    async fn diagnostics_for(
        &self,
        path: &Path,
        text: &str,
        wait: Duration,
    ) -> Result<Vec<Diagnostic>>;

    /// Best-effort shutdown. Called via `LspManager::shutdown_all`.
    #[allow(dead_code)]
    async fn shutdown(&self) -> bool;
}

/// Stdio-backed transport. Spawns the LSP server as a child process and
/// pipes JSON-RPC over stdin/stdout. Stderr is captured into a buffer so
/// callers can include it in error messages without polluting our own stderr.
pub struct StdioLspTransport {
    /// OS-level ownership of the server and every descendant it spawns.
    /// Declared before `child` so the process tree is killed first on Drop.
    process_tree: AsyncMutex<ProcessTreeOwner>,
    /// JoinHandle for the running server. Held so the child stays alive for
    /// the transport's lifetime; consumed during `shutdown`.
    #[allow(dead_code)]
    child: AsyncMutex<Option<Child>>,
    /// Outgoing message sender to the writer task.
    tx_outbound: mpsc::Sender<Vec<u8>>,
    /// Reader/writer/dispatcher owners. Shutdown aborts and joins every task
    /// after the child process is reaped, so Headless terminal publication
    /// cannot race an orphaned LSP tail.
    tasks: AsyncMutex<Vec<JoinHandle<()>>>,
    /// Synchronous Drop backstop. `JoinHandle` itself detaches on Drop, so
    /// retain abort capabilities independently of the async join owner.
    task_abort_handles: Vec<tokio::task::AbortHandle>,
    /// Inbound diagnostics queue. We push every `publishDiagnostics`
    /// notification into here and the public API drains the relevant entries.
    diagnostics_rx: AsyncMutex<mpsc::Receiver<(PathBuf, Vec<Diagnostic>)>>,
    /// Map of in-flight request id -> reply slot. We do not currently call
    /// methods that need replies after `initialize`, but this is the hook
    /// for it.
    #[allow(dead_code)]
    pending: Arc<AsyncMutex<HashMap<i64, oneshot::Sender<Value>>>>,
    /// Monotonic request id counter. Reserved for future LSP request/reply
    /// methods (workspace symbol queries, etc.).
    #[allow(dead_code)]
    next_id: AsyncMutex<i64>,
    /// Language id passed in `textDocument/didOpen` (e.g. "rust").
    language_id: String,
    /// Track which files we have opened so the second touch sends
    /// `didChange` instead of `didOpen`.
    opened: AsyncMutex<HashMap<PathBuf, i64>>,
}

impl StdioLspTransport {
    /// Spawn `command args…` and run the LSP `initialize` handshake. Returns
    /// `Err` immediately if the binary is not on PATH or `initialize` fails.
    pub async fn spawn(
        command: &str,
        args: &[String],
        language_id: &str,
        workspace: PathBuf,
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);
        configure_process_tree(cmd.as_std_mut());

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn LSP server `{command}`"))?;
        let process_tree =
            match ProcessTreeOwner::attach_tokio(&child, &format!("LSP server {command}")) {
                Ok(owner) => owner,
                Err(error) => {
                    let _ = child.start_kill();
                    let _ = tokio::time::timeout(LSP_SHUTDOWN_GRACE, child.wait()).await;
                    return Err(error).with_context(|| {
                        format!("failed to own LSP process tree for `{command}`")
                    });
                }
            };

        let stdin = child
            .stdin
            .take()
            .context("LSP child has no stdin handle")?;
        let stdout = child
            .stdout
            .take()
            .context("LSP child has no stdout handle")?;

        let (tx_outbound, rx_outbound) = mpsc::channel::<Vec<u8>>(64);
        let (tx_inbound, rx_inbound) = mpsc::channel::<Value>(64);
        let (tx_diag, rx_diag) = mpsc::channel::<(PathBuf, Vec<Diagnostic>)>(64);

        // Writer task: drain outbound channel, frame with Content-Length, write to stdin.
        let writer = spawn_supervised(
            "lsp-writer",
            std::panic::Location::caller(),
            writer_task(stdin, rx_outbound),
        );
        // Reader task: parse Content-Length frames from stdout, push to inbound queue.
        let reader = spawn_supervised(
            "lsp-reader",
            std::panic::Location::caller(),
            reader_task(stdout, tx_inbound),
        );
        // Inbound dispatcher: routes notifications to `tx_diag`, replies to a
        // pending map. We keep the pending map for completeness even though
        // diagnostics polling itself does not reuse it.
        let pending: Arc<AsyncMutex<HashMap<i64, oneshot::Sender<Value>>>> =
            Arc::new(AsyncMutex::new(HashMap::new()));
        let dispatcher = spawn_supervised(
            "lsp-dispatcher",
            std::panic::Location::caller(),
            dispatcher_task(rx_inbound, tx_diag, pending.clone()),
        );

        let tasks = vec![writer, reader, dispatcher];
        let task_abort_handles = tasks.iter().map(JoinHandle::abort_handle).collect();
        let transport = Self {
            process_tree: AsyncMutex::new(process_tree),
            child: AsyncMutex::new(Some(child)),
            tx_outbound,
            tasks: AsyncMutex::new(tasks),
            task_abort_handles,
            diagnostics_rx: AsyncMutex::new(rx_diag),
            pending,
            next_id: AsyncMutex::new(2),
            language_id: language_id.to_string(),
            opened: AsyncMutex::new(HashMap::new()),
        };

        // Send `initialize` and wait for `initialized`. We synthesize id=1.
        let init_payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": std::process::id(),
                "rootUri": uri_from_path(&workspace),
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": { "relatedInformation": false }
                    }
                },
                "workspaceFolders": [{
                    "uri": uri_from_path(&workspace),
                    "name": "workspace"
                }]
            }
        });
        let handshake = async {
            send_message(&transport.tx_outbound, &init_payload).await?;

            // We do not actually wait for the initialize response here in MVP —
            // most servers buffer notifications until they are ready, and waiting
            // for `initialize` reply doubles the latency of the first edit. Send
            // `initialized` immediately and let publishDiagnostics arrive on its
            // own clock.
            let initialized = json!({
                "jsonrpc": "2.0",
                "method": "initialized",
                "params": {}
            });
            send_message(&transport.tx_outbound, &initialized).await
        }
        .await;
        if let Err(error) = handshake {
            let _ = transport.shutdown().await;
            return Err(error);
        }

        Ok(transport)
    }
}

#[async_trait]
impl LspTransport for StdioLspTransport {
    async fn diagnostics_for(
        &self,
        path: &Path,
        text: &str,
        wait: Duration,
    ) -> Result<Vec<Diagnostic>> {
        let path_buf = path.to_path_buf();
        let uri = uri_from_path(&path_buf);

        // Either send didOpen (first time) or didChange (subsequent edits).
        let mut opened = self.opened.lock().await;
        let is_new = !opened.contains_key(&path_buf);
        let new_version = opened.get(&path_buf).copied().unwrap_or(0) + 1;
        opened.insert(path_buf.clone(), new_version);
        drop(opened);

        let payload = if is_new {
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri.clone(),
                        "languageId": self.language_id,
                        "version": new_version,
                        "text": text
                    }
                }
            })
        } else {
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": {
                        "uri": uri.clone(),
                        "version": new_version
                    },
                    "contentChanges": [{ "text": text }]
                }
            })
        };
        send_message(&self.tx_outbound, &payload).await?;

        // Drain matching `publishDiagnostics` notifications until `wait`
        // elapses. Servers typically publish within a few hundred ms; for
        // initial cold-start (rust-analyzer) it can be many seconds — but
        // the manager guards us with a separate timeout.
        let deadline = tokio::time::Instant::now() + wait;
        let mut latest: Option<Vec<Diagnostic>> = None;

        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                break;
            }
            let remaining = deadline - now;
            let mut rx = self.diagnostics_rx.lock().await;
            let next = match timeout(remaining, rx.recv()).await {
                Ok(Some(item)) => item,
                Ok(None) => break, // channel closed
                Err(_) => break,   // timed out
            };
            drop(rx);
            let (file, items) = next;
            if file == path_buf {
                latest = Some(items);
                // We have a payload — return immediately. If the server
                // re-publishes after rapid edits, the next call will sync.
                break;
            }
            // Otherwise: notification was for a different file we previously
            // opened. Discard and continue waiting.
        }
        Ok(latest.unwrap_or_default())
    }

    async fn shutdown(&self) -> bool {
        let mut settled = true;
        let mut child = self.child.lock().await;
        if let Some(mut c) = child.take() {
            let mut process_tree = self.process_tree.lock().await;
            if !shutdown_tokio_process_tree(&mut c, &mut process_tree, LSP_SHUTDOWN_GRACE).await {
                tracing::warn!("LSP child did not reap within the hard shutdown bound");
                settled = false;
            }
        }
        drop(child);

        let mut tasks = {
            let mut guard = self.tasks.lock().await;
            std::mem::take(&mut *guard)
        };
        for task in &tasks {
            if !task.is_finished() {
                task.abort();
            }
        }
        let deadline = tokio::time::Instant::now() + LSP_TASK_JOIN_GRACE;
        for mut task in tasks.drain(..) {
            match tokio::time::timeout_at(deadline, &mut task).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) if error.is_cancelled() => {}
                Ok(Err(error)) => {
                    settled = false;
                    tracing::warn!(?error, "LSP lifecycle task failed during shutdown");
                }
                Err(_) => settled = false,
            }
        }
        settled
    }
}

impl Drop for StdioLspTransport {
    fn drop(&mut self) {
        if let Ok(process_tree) = self.process_tree.try_lock() {
            let _ = process_tree.kill();
        }
        if let Ok(mut child) = self.child.try_lock()
            && let Some(child) = child.as_mut()
        {
            let _ = child.start_kill();
        }
        for handle in &self.task_abort_handles {
            handle.abort();
        }
    }
}

/// Send a JSON value as one Content-Length-framed JSON-RPC message.
async fn send_message(tx: &mpsc::Sender<Vec<u8>>, value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value).context("serialize LSP message")?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut frame = Vec::with_capacity(header.len() + body.len());
    frame.extend_from_slice(header.as_bytes());
    frame.extend_from_slice(&body);
    tx.send(frame)
        .await
        .map_err(|_| anyhow!("LSP outbound channel closed"))?;
    Ok(())
}

/// Background task that drains the outbound queue and writes each frame to
/// the LSP server's stdin. Exits cleanly when the channel closes.
async fn writer_task(mut stdin: tokio::process::ChildStdin, mut rx: mpsc::Receiver<Vec<u8>>) {
    while let Some(frame) = rx.recv().await {
        if stdin.write_all(&frame).await.is_err() {
            break;
        }
        if stdin.flush().await.is_err() {
            break;
        }
    }
}

/// Background task that parses `Content-Length`-framed JSON-RPC frames from
/// the LSP server's stdout. Pushes each parsed JSON value to `tx`. Exits
/// when stdout closes or a frame is malformed (we choose to fail closed
/// rather than risk hanging).
async fn reader_task(mut stdout: tokio::process::ChildStdout, tx: mpsc::Sender<Value>) {
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut tmp = [0u8; 4096];
    loop {
        let n = match stdout.read(&mut tmp).await {
            Ok(0) => return,
            Ok(n) => n,
            Err(_) => return,
        };
        buf.extend_from_slice(&tmp[..n]);
        // Try to parse as many frames as we can from the accumulated buffer.
        while let Some((header_end, content_length)) = parse_header(&buf) {
            if buf.len() < header_end + content_length {
                break; // need more bytes
            }
            let body = &buf[header_end..header_end + content_length];
            let parsed = serde_json::from_slice::<Value>(body).ok();
            // Drop the consumed bytes regardless of parse result so a bad frame
            // does not stall the loop.
            buf.drain(..header_end + content_length);
            if let Some(value) = parsed
                && tx.send(value).await.is_err()
            {
                return;
            }
        }
    }
}

/// Parse a JSON-RPC header block. Returns `Some((header_end, content_length))`
/// where `header_end` is the byte offset of the first body byte. The header
/// terminator is `\r\n\r\n`. We require a `Content-Length` header.
fn parse_header(buf: &[u8]) -> Option<(usize, usize)> {
    let term = b"\r\n\r\n";
    let pos = buf.windows(term.len()).position(|window| window == term)?;
    let header = std::str::from_utf8(&buf[..pos]).ok()?;
    let mut content_length: Option<usize> = None;
    for line in header.split("\r\n") {
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse::<usize>().ok();
        }
    }
    content_length.map(|cl| (pos + term.len(), cl))
}

/// Background task that consumes inbound JSON values, classifies them as
/// notifications/responses, and routes accordingly.
async fn dispatcher_task(
    mut rx: mpsc::Receiver<Value>,
    tx_diag: mpsc::Sender<(PathBuf, Vec<Diagnostic>)>,
    pending: Arc<AsyncMutex<HashMap<i64, oneshot::Sender<Value>>>>,
) {
    while let Some(value) = rx.recv().await {
        // Notifications have a `method` and no `id`.
        let method = value.get("method").and_then(|v| v.as_str());
        if method == Some("textDocument/publishDiagnostics") {
            if let Some((path, diags)) = parse_publish_diagnostics(&value) {
                let _ = tx_diag.send((path, diags)).await;
            }
            continue;
        }
        // Replies have an `id` and a `result` or `error`.
        if let Some(id) = value.get("id").and_then(|v| v.as_i64()) {
            let mut map = pending.lock().await;
            if let Some(slot) = map.remove(&id) {
                let _ = slot.send(value);
            }
        }
    }
}

/// Decode a `textDocument/publishDiagnostics` notification.
fn parse_publish_diagnostics(value: &Value) -> Option<(PathBuf, Vec<Diagnostic>)> {
    let params = value.get("params")?;
    let uri = params.get("uri")?.as_str()?;
    let path = path_from_uri(uri)?;
    let raw = params.get("diagnostics")?.as_array()?;
    let mut out = Vec::with_capacity(raw.len());
    for d in raw {
        let range = d.get("range")?;
        let start = range.get("start")?;
        let line = start.get("line")?.as_u64()? as u32 + 1;
        let column = start.get("character")?.as_u64()? as u32 + 1;
        let severity = Severity::from_lsp(d.get("severity").and_then(|v| v.as_i64()))
            .unwrap_or(Severity::Error);
        let message = d
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        out.push(Diagnostic {
            line,
            column,
            severity,
            message,
        });
    }
    Some((path, out))
}

/// Convert a filesystem path to a `file://` URI. Best-effort — we do not
/// support Windows drive letters perfectly, but the LSP servers in our
/// registry accept percent-encoded paths well enough for the post-edit
/// diagnostics use case.
fn uri_from_path(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = canonical.to_string_lossy();
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{}", s.trim_start_matches('/'))
    }
}

/// Inverse of [`uri_from_path`]. Returns `None` when the URI is not a `file://`.
fn path_from_uri(uri: &str) -> Option<PathBuf> {
    let stripped = uri.strip_prefix("file://")?;
    Some(PathBuf::from(stripped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn process_exists(pid: libc::pid_t) -> bool {
        let status = unsafe { libc::kill(pid, 0) };
        status == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    #[cfg(unix)]
    async fn wait_for_pid_file(path: &Path, wait: Duration) -> libc::pid_t {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if let Ok(raw) = tokio::fs::read_to_string(path).await
                && let Ok(pid) = raw.trim().parse::<libc::pid_t>()
            {
                return pid;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "process did not publish pid at {}",
                path.display()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[cfg(unix)]
    async fn wait_for_process_exit(pid: libc::pid_t, wait: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + wait;
        while process_exists(pid) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        !process_exists(pid)
    }

    #[test]
    fn parses_lsp_header() {
        let frame = b"Content-Length: 5\r\n\r\nhello";
        let (end, len) = parse_header(frame).expect("header parses");
        assert_eq!(end, 21);
        assert_eq!(len, 5);
    }

    #[test]
    fn parse_header_returns_none_when_truncated() {
        let frame = b"Content-Length: 5\r\nMissingTerm";
        assert!(parse_header(frame).is_none());
    }

    #[test]
    fn parses_publish_diagnostics_payload() {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": "file:///tmp/foo.rs",
                "diagnostics": [
                    {
                        "range": {
                            "start": { "line": 11, "character": 7 },
                            "end":   { "line": 11, "character": 8 }
                        },
                        "severity": 1,
                        "message": "missing semicolon"
                    }
                ]
            }
        });
        let (path, diags) = parse_publish_diagnostics(&payload).expect("parses");
        assert_eq!(path, PathBuf::from("/tmp/foo.rs"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].line, 12);
        assert_eq!(diags[0].column, 8);
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].message, "missing semicolon");
    }

    #[test]
    fn round_trips_uri_path() {
        let path = PathBuf::from("/tmp/example/foo.rs");
        let uri = format!("file://{}", path.display());
        assert_eq!(path_from_uri(&uri), Some(path));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stdio_lsp_shutdown_reaps_stubborn_descendant_tree() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let script_path = tmp.path().join("stubborn-lsp-tree.sh");
        let child_pid_path = tmp.path().join("child.pid");
        let term_marker_path = tmp.path().join("term-markers");
        std::fs::write(
            &script_path,
            r#"#!/bin/sh
pid_file=$1
term_file=$2
trap 'echo parent >> "$term_file"' TERM
(
  trap 'echo child >> "$term_file"' TERM
  while :; do sleep 1; done
) &
child=$!
echo "$child" > "$pid_file"
while :; do wait "$child"; done
"#,
        )
        .expect("write LSP lifecycle fixture");
        let args = vec![
            script_path.display().to_string(),
            child_pid_path.display().to_string(),
            term_marker_path.display().to_string(),
        ];

        let transport =
            StdioLspTransport::spawn("/bin/sh", &args, "lifecycle-test", tmp.path().to_path_buf())
                .await
                .expect("spawn production LSP transport");
        let parent_pid = transport
            .child
            .lock()
            .await
            .as_ref()
            .and_then(Child::id)
            .expect("LSP parent pid") as libc::pid_t;
        let child_pid = wait_for_pid_file(&child_pid_path, Duration::from_secs(3)).await;

        let started = std::time::Instant::now();
        tokio::time::timeout(Duration::from_secs(5), transport.shutdown())
            .await
            .expect("LSP tree shutdown exceeded Headless grace");
        assert!(
            started.elapsed() >= LSP_SHUTDOWN_GRACE,
            "fixture exited before exercising the hard-kill path: {:?}",
            started.elapsed()
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "LSP shutdown exceeded lifecycle bound: {:?}",
            started.elapsed()
        );
        let markers = std::fs::read_to_string(&term_marker_path).unwrap_or_default();
        assert!(markers.lines().any(|line| line == "parent"), "{markers:?}");
        assert!(markers.lines().any(|line| line == "child"), "{markers:?}");
        let parent_exited = wait_for_process_exit(parent_pid, Duration::from_secs(2)).await;
        let child_exited = wait_for_process_exit(child_pid, Duration::from_secs(2)).await;
        if !parent_exited || !child_exited {
            unsafe {
                libc::kill(-parent_pid, libc::SIGKILL);
            }
            panic!("LSP process tree survived shutdown: parent={parent_pid} child={child_pid}");
        }
        assert!(transport.child.lock().await.is_none());
        assert!(transport.tasks.lock().await.is_empty());
    }
}
