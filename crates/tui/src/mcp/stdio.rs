use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex as TokioMutex;

use super::{McpServerConfig, McpTransport};
use crate::child_env;
use crate::tools::shell::{ProcessTreeOwner, configure_process_tree, shutdown_tokio_process_tree};

pub(super) struct StdioTransport {
    pub(super) process_tree: ProcessTreeOwner,
    pub(super) child: Child,
    pub(super) stdin: ChildStdin,
    pub(super) reader: tokio::io::BufReader<ChildStdout>,
    /// Tail of stderr lines from the spawned MCP server. A background task
    /// drains the child's stderr into this buffer so a mid-run crash leaves
    /// some context behind instead of `Stdio::null` swallowing it.
    pub(super) stderr_tail: Arc<StderrTail>,
    pub(super) stderr_task: Option<tokio::task::JoinHandle<()>>,
}

/// How long `StdioTransport::shutdown` waits for the child to exit on SIGTERM
/// before explicitly starting a hard kill. Tuned short so a hung MCP server
/// cannot stall exit; well-behaved servers almost always exit within a few
/// hundred milliseconds.
pub(super) const STDIO_SHUTDOWN_GRACE: Duration = Duration::from_millis(1_000);

/// How many lines of MCP-server stderr to keep around for crash diagnostics.
/// Bounded so a chatty server can't grow this without limit; large enough to
/// catch typical Node/Python startup or panic output.
const STDERR_TAIL_CAPACITY: usize = 64;

/// The child has already exited (or received a hard kill) when this wait
/// begins, so stderr should reach EOF quickly. Keep a bound in case an OS pipe
/// or custom runtime fails to wake the drain task.
const STDERR_DRAIN_SHUTDOWN_GRACE: Duration = Duration::from_millis(250);

/// Bounded ring buffer for the most recent stderr lines from a spawned MCP
/// server. Used by `StdioTransport` to surface server-side context when the
/// transport read side fails (server crashed, exited early, etc).
#[derive(Default)]
pub(super) struct StderrTail {
    lines: TokioMutex<VecDeque<String>>,
}

impl StderrTail {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            lines: TokioMutex::new(VecDeque::with_capacity(STDERR_TAIL_CAPACITY)),
        })
    }

    pub(super) async fn push(&self, line: String) {
        let mut buf = self.lines.lock().await;
        if buf.len() >= STDERR_TAIL_CAPACITY {
            buf.pop_front();
        }
        buf.push_back(line);
    }

    async fn snapshot(&self) -> Vec<String> {
        self.lines.lock().await.iter().cloned().collect()
    }
}

impl StdioTransport {
    pub(super) fn spawn(
        server_name: &str,
        command: &str,
        config: &McpServerConfig,
    ) -> Result<Self> {
        let mut cmd = tokio::process::Command::new(command);
        crate::utils::suppress_tokio_console_window(&mut cmd);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        configure_process_tree(cmd.as_std_mut());
        if let Some(cwd) = &config.cwd {
            cmd.current_dir(cwd);
        }

        // Expand `${NAME}` placeholders so secret env values can be sourced
        // from the process environment instead of being stored in cleartext
        // in the MCP config. The child env is allowlist-sanitized below, so
        // these vars would not otherwise be inherited by the child.
        let expanded_env = super::expand_env_placeholders_map(&config.env, "env")
            .with_context(|| format!("MCP server '{server_name}' env expansion failed"))?;

        // MCP stdio servers are user-configured integrations. Use the
        // wider MCP allowlist so common Node/Python/proxy/CA-bundle
        // bootstrap variables (NVM_DIR, NODE_OPTIONS, NPM_CONFIG_*,
        // HTTP(S)_PROXY, …) reach the child. See `sanitized_mcp_env`
        // and #1244 for context.
        child_env::apply_to_tokio_command_mcp(&mut cmd, child_env::string_map_env(&expanded_env));

        let mut child = cmd.spawn().with_context(|| {
            let env_keys: Vec<&str> = expanded_env.keys().map(String::as_str).collect();
            format!(
                "MCP stdio spawn failed (transport=stdio server={server_name} cmd={command:?} args={:?} env_keys={env_keys:?})",
                config.args,
            )
        })?;
        let process_tree = match ProcessTreeOwner::attach_tokio(
            &child,
            &format!("MCP stdio server {server_name}"),
        ) {
            Ok(owner) => owner,
            Err(error) => {
                // A transport without process-tree ownership is not admitted.
                // Best-effort direct cleanup covers the attach-failure path;
                // Windows still has an inherent spawn-before-Job race unless
                // the platform spawn is later changed to suspended creation.
                let _ = child.start_kill();
                return Err(error).with_context(|| {
                    format!("failed to own MCP stdio process tree for '{server_name}'")
                });
            }
        };

        let stdin = child.stdin.take().context("Failed to get MCP stdin")?;
        let stdout = child.stdout.take().context("Failed to get MCP stdout")?;
        let stderr = child.stderr.take().context("Failed to get MCP stderr")?;

        // Drain stderr into a bounded ring buffer so a crash mid-run leaves
        // diagnostic breadcrumbs instead of disappearing into `Stdio::null`.
        // The task exits naturally when the child closes its stderr
        // (kill_on_drop / exit / explicit shutdown).
        let stderr_tail = StderrTail::new();
        let stderr_task = {
            let tail = Arc::clone(&stderr_tail);
            tokio::spawn(async move {
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tail.push(line).await;
                }
            })
        };

        Ok(Self {
            process_tree,
            child,
            stdin,
            reader: tokio::io::BufReader::new(stdout),
            stderr_tail,
            stderr_task: Some(stderr_task),
        })
    }

    async fn settle_stderr_task(&mut self) -> bool {
        let Some(mut task) = self.stderr_task.take() else {
            return true;
        };

        match tokio::time::timeout(STDERR_DRAIN_SHUTDOWN_GRACE, &mut task).await {
            Ok(Ok(())) => true,
            Ok(Err(error)) => error.is_cancelled(),
            Err(_) => {
                task.abort();
                match tokio::time::timeout(STDERR_DRAIN_SHUTDOWN_GRACE, &mut task).await {
                    Ok(Ok(())) => true,
                    Ok(Err(error)) => error.is_cancelled(),
                    Err(_) => false,
                }
            }
        }
    }
}

/// Format the captured stderr tail for inclusion in an error message. Empty
/// tails return `None` so the caller can fall back to its original message.
async fn format_stderr_context(tail: &StderrTail) -> Option<String> {
    let lines = tail.snapshot().await;
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "MCP server stderr (last {} line{}):\n{}",
        lines.len(),
        if lines.len() == 1 { "" } else { "s" },
        lines.join("\n"),
    ))
}

#[async_trait::async_trait]
impl McpTransport for StdioTransport {
    async fn send(&mut self, mut msg: Vec<u8>) -> Result<()> {
        msg.push(b'\n');
        self.stdin.write_all(&msg).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Vec<u8>> {
        let mut line_bytes: Vec<u8> = Vec::new();
        loop {
            // Bounded read: a server emitting a newline-free multi-GB "line"
            // must not OOM us (read_line is unbounded).
            let bytes = match read_line_capped(
                &mut self.reader,
                &mut line_bytes,
                super::MAX_MCP_RESPONSE_BYTES,
            )
            .await
            {
                Ok(b) => b,
                Err(err) => {
                    if let Some(stderr) = format_stderr_context(&self.stderr_tail).await {
                        anyhow::bail!("Stdio transport read error: {err}\n{stderr}");
                    }
                    return Err(err.into());
                }
            };
            if bytes == 0 {
                if let Some(stderr) = format_stderr_context(&self.stderr_tail).await {
                    anyhow::bail!("Stdio transport closed\n{stderr}");
                }
                anyhow::bail!("Stdio transport closed");
            }

            let line = String::from_utf8_lossy(&line_bytes);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            return Ok(trimmed.as_bytes().to_vec());
        }
    }

    /// Send SIGTERM and wait up to `STDIO_SHUTDOWN_GRACE` for graceful exit.
    /// A child that does not exit is killed and reaped before the retained
    /// stderr drain task is joined (or aborted after its own short bound).
    async fn shutdown(&mut self) -> bool {
        let _ = self.stdin.shutdown().await;
        let tree_settled = shutdown_tokio_process_tree(
            &mut self.child,
            &mut self.process_tree,
            STDIO_SHUTDOWN_GRACE,
        )
        .await;
        if !tree_settled {
            tracing::warn!("MCP stdio child did not reap within the hard shutdown bound");
        }
        let stderr_settled = self.settle_stderr_task().await;
        if !stderr_settled {
            tracing::warn!("MCP stdio stderr drain task did not settle within its shutdown bound");
        }
        tree_settled && stderr_settled
    }
}

/// Drop fallback: if async shutdown was skipped, hard-kill the full owned
/// process tree before Tokio's direct-child `kill_on_drop` backstop.
impl Drop for StdioTransport {
    fn drop(&mut self) {
        let _ = self.process_tree.kill();
        let _ = self.child.start_kill();
        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }
    }
}

/// Read one newline-terminated line into `out` (cleared first), aborting if it
/// exceeds `max` bytes without a newline. Bounds an otherwise-unbounded
/// `read_line` so a misbehaving MCP server cannot OOM the client. Returns the
/// number of bytes accumulated; 0 means EOF.
async fn read_line_capped<R>(
    reader: &mut R,
    out: &mut Vec<u8>,
    max: usize,
) -> std::io::Result<usize>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;
    out.clear();
    loop {
        let (chunk, consumed, done) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                (Vec::new(), 0usize, true)
            } else if let Some(pos) = available.iter().position(|&b| b == b'\n') {
                (available[..=pos].to_vec(), pos + 1, true)
            } else {
                (available.to_vec(), available.len(), false)
            }
        };
        if consumed > 0 {
            reader.consume(consumed);
        }
        out.extend_from_slice(&chunk);
        if done {
            break;
        }
        if out.len() > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("MCP stdio line exceeded {max} bytes without a newline"),
            ));
        }
    }
    Ok(out.len())
}

#[cfg(test)]
mod read_cap_tests {
    use super::read_line_capped;

    #[tokio::test]
    async fn reads_a_line_and_reports_eof() {
        let data = b"hello\nworld\n".to_vec();
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(data));
        let mut out = Vec::new();
        assert_eq!(
            read_line_capped(&mut reader, &mut out, 1024).await.unwrap(),
            6
        );
        assert_eq!(out, b"hello\n");
        assert_eq!(
            read_line_capped(&mut reader, &mut out, 1024).await.unwrap(),
            6
        );
        assert_eq!(out, b"world\n");
        // EOF.
        assert_eq!(
            read_line_capped(&mut reader, &mut out, 1024).await.unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn aborts_on_newline_free_line_over_cap() {
        let data = vec![b'x'; 4096]; // no newline
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(data));
        let mut out = Vec::new();
        let err = read_line_capped(&mut reader, &mut out, 1024)
            .await
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }
}
