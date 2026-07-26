//! Canonical managed process execution for production tools.
//!
//! `exec_shell`, `run_tests`, and `run_verifiers` share this internal owner so
//! timeout, cancellation, process-tree termination, sandboxing, and output
//! capture have one implementation. The model-visible shell tool is strictly
//! foreground-only; this module does not expose a background-job product.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
#[cfg(windows)]
use windows::core::PCWSTR;

pub(crate) mod cargo_failure_summary;
mod exec_shell;
mod output;

pub(crate) use exec_shell::{
    ExecShellHost, ExecShellOptions, ExecShellPolicyDecision, command_likely_needs_network,
    execute_exec_shell, execute_managed_program, preflight_exec_shell,
};

use crate::child_env;
use crate::sandbox::{
    CommandSpec, ExecEnv, SandboxManager, SandboxPolicy as ExecutionSandboxPolicy, SandboxType,
};
use output::truncate_with_meta;

#[cfg(windows)]
fn suppress_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn suppress_console_window(_command: &mut Command) {}

/// Effective shell access for one tool-execution context.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ShellPolicy {
    /// No shell access.
    None,
    /// Only commands classified as non-mutating foreground inspection.
    ReadOnly,
    /// Full shell access.
    Full,
}

/// Status of a shell process
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum ShellStatus {
    Running,
    Completed,
    Failed,
    Killed,
    TimedOut,
}

/// Result from a shell command execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ShellResult {
    pub status: ShellStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    /// Original stdout length in bytes.
    #[serde(default)]
    pub stdout_len: usize,
    /// Original stderr length in bytes.
    #[serde(default)]
    pub stderr_len: usize,
    /// Bytes omitted from stdout due to truncation.
    #[serde(default)]
    pub stdout_omitted: usize,
    /// Bytes omitted from stderr due to truncation.
    #[serde(default)]
    pub stderr_omitted: usize,
    /// Whether stdout was truncated.
    #[serde(default)]
    pub stdout_truncated: bool,
    /// Whether stderr was truncated.
    #[serde(default)]
    pub stderr_truncated: bool,
    /// Whether the command was executed in a sandbox.
    #[serde(default)]
    pub sandboxed: bool,
    /// Type of sandbox used (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_type: Option<String>,
    /// Whether the command was blocked by sandbox restrictions.
    #[serde(default)]
    pub sandbox_denied: bool,
}

#[cfg(unix)]
fn kill_child_process_group(child: &mut Child) -> std::io::Result<()> {
    let pgid = child.id() as libc::pid_t;
    if pgid <= 0 {
        return child.kill();
    }

    let result = unsafe { libc::kill(-pgid, libc::SIGKILL) };
    if result == 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            child.kill()
        }
    }
}

/// Configure parent-death signaling so shell-spawned children are reaped when
/// the TUI dies abnormally (#421). On Linux this installs
/// `PR_SET_PDEATHSIG(SIGTERM)` via `pre_exec` — the kernel then sends SIGTERM
/// to the child the moment the parent process exits, even on SIGKILL of the
/// TUI. The cancellation path already SIGKILLs the whole process group, so
/// this only fires when the parent dies without running its drop / cleanup
/// code (panic during shutdown, OOM, hardware crash, etc.).
///
/// On macOS / Windows there's no kernel equivalent. The existing graceful
/// path (`kill_child_process_group` from the cancellation token) still
/// handles normal shutdown; abnormal exit can leak children — tracked as a
/// follow-up watchdog item per the original issue's acceptance criteria.
#[cfg(all(target_os = "linux", not(target_env = "ohos")))]
fn install_parent_death_signal(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: `pre_exec` runs in the child between fork and exec. The closure
    // only calls `libc::prctl` with stack-allocated constant arguments and
    // does not touch heap memory or the parent's locks. Both requirements
    // (async-signal-safe + no allocation in the post-fork window) are met.
    unsafe {
        cmd.pre_exec(|| {
            let result = libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0);
            if result == -1 {
                // Surface the errno but do not abort the spawn — the child
                // will simply lose the parent-death cleanup safety net.
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

/// Attach `args` to a `std::process::Command`, honoring shell-quoting on
/// Windows.
///
/// Issue #1691: on Windows the shell command is invoked as
/// `cmd /C "chcp 65001 >NUL & <command>"`. Rust's `Command::arg` applies
/// MSVCRT (`CommandLineToArgvW`) escaping, turning the embedded `"` in a
/// quoted argument (e.g. `git commit -m "feat: complete sub-pages"`) into
/// `\"`. `cmd.exe` does NOT use MSVCRT parsing — it treats `\` literally and
/// `"` as a bare quote toggle — so the escaped payload is mis-tokenized and
/// `git` receives `feat:`, `complete`, `sub-pages"` as separate pathspecs
/// (the reported `pathspec 'sub-pages"' did not match` symptom). Passing the
/// `cmd /C` payload through `CommandExt::raw_arg` suppresses std's escaping so
/// the string reaches `cmd.exe` verbatim, exactly as a terminal would.
#[cfg(windows)]
fn push_shell_args(cmd: &mut Command, program: &str, args: &[String]) {
    use std::os::windows::process::CommandExt;
    // The `cmd /C <payload>` shape is the only place std's per-arg escaping
    // corrupts a quoted command. Pass `/C` and the payload raw so the quotes
    // survive; any other program keeps normal (correct) escaping. Match `cmd`
    // by file stem so a full path (`C:\Windows\System32\cmd.exe`) or `.exe`
    // suffix still triggers the raw-arg path.
    let is_cmd = std::path::Path::new(program)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("cmd"))
        .unwrap_or(false);
    if is_cmd && args.len() == 2 && args[0].eq_ignore_ascii_case("/C") {
        cmd.raw_arg(&args[0]);
        cmd.raw_arg(&args[1]);
    } else {
        cmd.args(args);
    }
}

#[cfg(not(windows))]
fn push_shell_args(cmd: &mut Command, _program: &str, args: &[String]) {
    // Unix delegates tokenization entirely to `sh -c <command>`; the command
    // string is passed as a single argv entry and never split by us.
    cmd.args(args);
}

#[cfg(not(all(target_os = "linux", not(target_env = "ohos"))))]
fn install_parent_death_signal(_cmd: &mut Command) {
    // No kernel-level equivalent on macOS / Windows. The cooperative
    // cancellation + process_group SIGKILL path covers normal shutdown;
    // abnormal exit (panic without unwind, SIGKILL of the TUI) can still
    // leak children on those platforms — tracked as a follow-up.
}

/// Put a spawned command in its own process tree before `spawn`.
///
/// Shell jobs, MCP stdio servers, and LSP servers all use this exact setup so
/// their descendants can be terminated as one owned unit. The Linux
/// parent-death signal remains a fallback for abnormal host termination.
pub fn configure_process_tree(command: &mut Command) {
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    install_parent_death_signal(command);
}

#[cfg(windows)]
#[derive(Debug)]
struct WindowsJob {
    handle: HANDLE,
}

#[cfg(windows)]
// SAFETY: Windows job handles are process-wide kernel handles. Moving the
// wrapper between threads does not invalidate the handle, and access is
// externally synchronized by ShellManager's mutex.
unsafe impl Send for WindowsJob {}
#[cfg(windows)]
// SAFETY: The wrapper exposes only terminate/drop operations around a kernel
// handle; concurrent use is guarded by ShellManager.
unsafe impl Sync for WindowsJob {}

#[cfg(windows)]
impl WindowsJob {
    fn attach_to_child(child: &Child) -> std::io::Result<Self> {
        Self::attach_to_process_handle(HANDLE(child.as_raw_handle()))
    }

    fn attach_to_process_handle(process_handle: HANDLE) -> std::io::Result<Self> {
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()).map_err(windows_io_error)? };
        let job = Self { handle };

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        unsafe {
            SetInformationJobObject(
                job.handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(windows_io_error)?;

            AssignProcessToJobObject(job.handle, process_handle).map_err(windows_io_error)?;
        }

        Ok(job)
    }

    fn terminate(&self) -> std::io::Result<()> {
        unsafe { TerminateJobObject(self.handle, 1).map_err(windows_io_error) }
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
fn windows_io_error(error: windows::core::Error) -> std::io::Error {
    std::io::Error::other(error)
}

#[cfg(windows)]
fn terminate_windows_job(job: Option<&WindowsJob>, child: &mut Child) -> std::io::Result<()> {
    if let Some(job) = job {
        match job.terminate() {
            Ok(()) => return Ok(()),
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "failed to terminate Windows job object; falling back to immediate child kill"
                );
            }
        }
    }
    child.kill()
}

#[cfg(windows)]
fn terminate_and_close_windows_job(windows_job: Option<WindowsJob>) {
    if let Some(job) = windows_job.as_ref()
        && let Err(err) = job.terminate()
    {
        tracing::warn!(
            ?err,
            "failed to terminate Windows shell job before closing job handle"
        );
    }
    drop(windows_job);
}

#[cfg(windows)]
fn terminate_child_and_close_windows_job(
    windows_job: Option<WindowsJob>,
    child: &mut Child,
) -> std::io::Result<()> {
    let result = terminate_windows_job(windows_job.as_ref(), child);
    drop(windows_job);
    result
}

#[cfg(windows)]
fn attach_windows_job(child: &Child, command: &str) -> Option<WindowsJob> {
    match WindowsJob::attach_to_child(child) {
        Ok(job) => Some(job),
        Err(error) => {
            tracing::warn!(
                ?error,
                command,
                "failed to attach Windows shell process to job object; descendant cleanup degraded"
            );
            None
        }
    }
}

/// Cross-platform owner for one directly spawned Tokio process and every
/// descendant it creates. Unix uses a dedicated process group; Windows reuses
/// the same kill-on-close Job Object implementation as shell jobs.
#[derive(Debug)]
pub struct ProcessTreeOwner {
    #[cfg(unix)]
    process_group_id: Option<libc::pid_t>,
    #[cfg(windows)]
    windows_job: Option<WindowsJob>,
}

impl ProcessTreeOwner {
    /// Attach lifecycle ownership to a standard-library child spawned from a
    /// command prepared with [`configure_process_tree`].
    pub fn attach_std(child: &Child, label: &str) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            let _ = label;
            Self::from_process_id(child.id())
        }

        #[cfg(windows)]
        {
            return Self::from_windows_handle(HANDLE(child.as_raw_handle()), label);
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = (child, label);
            Ok(Self {})
        }
    }

    /// Attach lifecycle ownership immediately after spawning a command that
    /// was prepared with [`configure_process_tree`].
    pub fn attach_tokio(child: &tokio::process::Child, label: &str) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            let _ = label;
            let pid = child.id().ok_or_else(|| {
                std::io::Error::other("spawned process has no live process identifier")
            })?;
            Self::from_process_id(pid)
        }

        #[cfg(windows)]
        {
            let raw_handle = child.raw_handle().ok_or_else(|| {
                std::io::Error::other(format!(
                    "spawned process {label} has no live Windows process handle"
                ))
            })?;
            return Self::from_windows_handle(HANDLE(raw_handle), label);
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = (child, label);
            Ok(Self {})
        }
    }

    #[cfg(unix)]
    fn from_process_id(pid: u32) -> std::io::Result<Self> {
        let process_group_id = libc::pid_t::try_from(pid)
            .map_err(|_| std::io::Error::other("process identifier exceeds pid_t"))?;
        Ok(Self {
            process_group_id: Some(process_group_id),
        })
    }

    #[cfg(windows)]
    fn from_windows_handle(process_handle: HANDLE, label: &str) -> std::io::Result<Self> {
        WindowsJob::attach_to_process_handle(process_handle)
            .map(|windows_job| Self {
                windows_job: Some(windows_job),
            })
            .map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("failed to attach {label} to Windows job object: {error}"),
                )
            })
    }

    /// Ask the whole owned tree to stop. Windows has no SIGTERM equivalent,
    /// so terminating the Job Object is necessarily immediate there.
    pub fn terminate(&self) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            self.signal_unix_group(libc::SIGTERM)
        }
        #[cfg(windows)]
        {
            if let Some(job) = self.windows_job.as_ref() {
                return job.terminate();
            }
            Ok(())
        }
        #[cfg(not(any(unix, windows)))]
        {
            Ok(())
        }
    }

    /// Force the whole owned tree to stop.
    pub fn kill(&self) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            self.signal_unix_group(libc::SIGKILL)
        }
        #[cfg(windows)]
        {
            if let Some(job) = self.windows_job.as_ref() {
                return job.terminate();
            }
            Ok(())
        }
        #[cfg(not(any(unix, windows)))]
        {
            Ok(())
        }
    }

    #[cfg(unix)]
    fn signal_unix_group(&self, signal: libc::c_int) -> std::io::Result<()> {
        let Some(process_group_id) = self.process_group_id else {
            return Ok(());
        };
        if process_group_id <= 0 {
            return Ok(());
        }
        let result = unsafe { libc::kill(-process_group_id, signal) };
        if result == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }

    pub fn disarm(&mut self) {
        #[cfg(unix)]
        {
            self.process_group_id = None;
        }
        #[cfg(windows)]
        {
            self.windows_job = None;
        }
    }
}

impl Drop for ProcessTreeOwner {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

/// Gracefully stop, force-kill, and reap one Tokio child process tree within
/// two bounded grace windows. Returns `true` only when the direct child was
/// reaped; the owner remains armed as a Drop fallback otherwise.
pub async fn shutdown_tokio_process_tree(
    child: &mut tokio::process::Child,
    owner: &mut ProcessTreeOwner,
    grace: Duration,
) -> bool {
    if child.id().is_none() {
        // The direct child may already be reaped while one of its descendants
        // still owns the process group / Job. Sweep the retained tree before
        // releasing its identity.
        let tree_settled = owner.kill().is_ok();
        if tree_settled {
            owner.disarm();
        }
        return tree_settled;
    }

    #[cfg(any(unix, windows))]
    let _ = owner.terminate();
    #[cfg(not(any(unix, windows)))]
    let _ = child.start_kill();

    if matches!(tokio::time::timeout(grace, child.wait()).await, Ok(Ok(_))) {
        // The direct server can exit while a descendant ignores SIGTERM.
        // Sweep the still-owned group/job before releasing its identity.
        let tree_settled = owner.kill().is_ok();
        if tree_settled {
            owner.disarm();
        }
        return tree_settled;
    }

    let tree_settled = owner.kill().is_ok();
    let _ = child.start_kill();
    let reaped = matches!(tokio::time::timeout(grace, child.wait()).await, Ok(Ok(_)));
    let settled = tree_settled && reaped;
    if settled {
        owner.disarm();
    }
    settled
}

fn spawn_reader_thread<R: Read + Send + 'static>(
    mut reader: R,
    buffer: Arc<Mutex<Vec<u8>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut guard) = buffer.lock() {
                        guard.extend_from_slice(&chunk[..n]);
                    }
                }
                Err(_) => break,
            }
        }
    })
}

/// One foreground tool process tracked while its async caller polls,
/// cancels, or times out.
struct ManagedProcess {
    status: ShellStatus,
    exit_code: Option<i32>,
    started_at: Instant,
    sandbox_type: SandboxType,
    stdout_buffer: Arc<Mutex<Vec<u8>>>,
    stderr_buffer: Arc<Mutex<Vec<u8>>>,
    child: Option<Child>,
    #[cfg(windows)]
    windows_job: Option<WindowsJob>,
    stdout_thread: Option<std::thread::JoinHandle<()>>,
    stderr_thread: Option<std::thread::JoinHandle<()>>,
}

impl ManagedProcess {
    fn poll(&mut self) -> bool {
        if self.status != ShellStatus::Running {
            return true;
        }

        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.exit_code = status.code();
                    self.status = if status.success() {
                        ShellStatus::Completed
                    } else {
                        ShellStatus::Failed
                    };
                    self.collect_output();
                    true
                }
                Ok(None) => false, // Still running
                Err(_) => {
                    self.status = ShellStatus::Failed;
                    self.collect_output();
                    true
                }
            }
        } else {
            true
        }
    }

    fn collect_output(&mut self) {
        // A shell may exit while descendants retain stdout/stderr pipe handles.
        // Sweep the process tree before joining readers so foreground completion
        // cannot hang indefinitely.
        #[cfg(unix)]
        if let Some(child) = self.child.as_mut() {
            let _ = kill_child_process_group(child);
        }
        #[cfg(windows)]
        terminate_and_close_windows_job(self.windows_job.take());
        if let Some(handle) = self.stdout_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
        self.child = None;
    }

    fn full_output(&self) -> (String, String, usize, usize) {
        let stdout_bytes = self
            .stdout_buffer
            .lock()
            .map(|data| data.clone())
            .unwrap_or_default();
        let stderr_bytes = self
            .stderr_buffer
            .lock()
            .map(|data| data.clone())
            .unwrap_or_default();

        let stdout_len = stdout_bytes.len();
        let stderr_len = stderr_bytes.len();

        (
            String::from_utf8_lossy(&stdout_bytes).to_string(),
            String::from_utf8_lossy(&stderr_bytes).to_string(),
            stdout_len,
            stderr_len,
        )
    }

    fn sandbox_denied(&self) -> bool {
        if matches!(self.status, ShellStatus::Running) {
            return false;
        }
        let (_, stderr_full, _, _) = self.full_output();
        SandboxManager::was_denied(
            self.sandbox_type,
            self.exit_code.unwrap_or(-1),
            &stderr_full,
        )
    }

    fn kill(&mut self) -> Result<()> {
        if let Some(ref mut child) = self.child {
            #[cfg(windows)]
            {
                terminate_windows_job(self.windows_job.as_ref(), child)
                    .context("Failed to kill process tree")?;
                let _ = child.wait();
            }
            #[cfg(not(windows))]
            {
                #[cfg(unix)]
                kill_child_process_group(child).context("Failed to kill process tree")?;
                #[cfg(not(unix))]
                child.kill().context("Failed to kill process")?;
                let _ = child.wait();
            }
        }
        self.status = ShellStatus::Killed;
        self.collect_output();
        Ok(())
    }

    fn snapshot(&self) -> ShellResult {
        let sandboxed = !matches!(self.sandbox_type, SandboxType::None);
        let (stdout_full, stderr_full, _, _) = self.full_output();
        let (stdout, stdout_meta) = truncate_with_meta(&stdout_full);
        let (stderr, stderr_meta) = truncate_with_meta(&stderr_full);
        ShellResult {
            status: self.status.clone(),
            exit_code: self.exit_code,
            stdout,
            stderr,
            duration_ms: u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
            stdout_len: stdout_meta.original_len,
            stderr_len: stderr_meta.original_len,
            stdout_omitted: stdout_meta.omitted,
            stderr_omitted: stderr_meta.omitted,
            stdout_truncated: stdout_meta.truncated,
            stderr_truncated: stderr_meta.truncated,
            sandboxed,
            sandbox_type: if sandboxed {
                Some(self.sandbox_type.to_string())
            } else {
                None
            },
            sandbox_denied: self.sandbox_denied(),
        }
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        if self.status == ShellStatus::Running {
            let _ = self.kill();
        }
    }
}

/// Owns only processes that are currently executing for a foreground tool.
pub(crate) struct ShellManager {
    processes: HashMap<String, ManagedProcess>,
    default_workspace: PathBuf,
    sandbox_manager: SandboxManager,
    sandbox_policy: ExecutionSandboxPolicy,
}

impl std::fmt::Debug for ShellManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellManager")
            .field("processes", &self.processes.len())
            .field("default_workspace", &self.default_workspace)
            .field("sandbox_policy", &self.sandbox_policy)
            .finish()
    }
}

impl ShellManager {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            processes: HashMap::new(),
            default_workspace: workspace,
            sandbox_manager: SandboxManager::new(),
            sandbox_policy: ExecutionSandboxPolicy::default(),
        }
    }

    /// Spawn a shell command under the shared managed-process lifecycle.
    fn spawn_shell(
        &mut self,
        command: &str,
        working_dir: Option<&str>,
        timeout_ms: u64,
        policy_override: Option<ExecutionSandboxPolicy>,
        extra_env: HashMap<String, String>,
    ) -> Result<String> {
        crate::shell_dispatcher::ShellDispatcher::log_exec(command);
        let work_dir = working_dir.map_or_else(|| self.default_workspace.clone(), PathBuf::from);
        let timeout_ms = timeout_ms.clamp(1_000, 600_000);
        let policy = policy_override.unwrap_or_else(|| self.sandbox_policy.clone());
        let spec = CommandSpec::shell(command, work_dir.clone(), Duration::from_millis(timeout_ms))
            .with_policy(policy)
            .with_env(extra_env);
        let exec_env = self.sandbox_manager.prepare_enforced(&spec)?;
        self.spawn_prepared(command, &work_dir, &exec_env)
    }

    /// Spawn a directly addressed verifier/test program under the same owner.
    #[allow(clippy::too_many_arguments)]
    fn spawn_managed_program(
        &mut self,
        display_command: &str,
        program: &str,
        args: &[String],
        working_dir: &Path,
        timeout_ms: u64,
        policy_override: Option<ExecutionSandboxPolicy>,
        extra_env: HashMap<String, String>,
    ) -> Result<String> {
        crate::shell_dispatcher::ShellDispatcher::log_exec(display_command);
        let timeout_ms = timeout_ms.clamp(1_000, 600_000);
        let policy = policy_override.unwrap_or_else(|| self.sandbox_policy.clone());
        let spec = CommandSpec::program(
            program,
            args.to_vec(),
            working_dir.to_path_buf(),
            Duration::from_millis(timeout_ms),
        )
        .with_policy(policy)
        .with_env(extra_env);
        let exec_env = self.sandbox_manager.prepare_enforced(&spec)?;
        self.spawn_prepared(display_command, working_dir, &exec_env)
    }

    fn spawn_prepared(
        &mut self,
        original_command: &str,
        working_dir: &Path,
        exec_env: &ExecEnv,
    ) -> Result<String> {
        let process_id = format!("process_{}", &Uuid::new_v4().to_string()[..8]);
        let started = Instant::now();
        let sandbox_type = exec_env.sandbox_type;
        let program = exec_env.program();
        let args = exec_env.args();
        let mut cmd = Command::new(program);
        suppress_console_window(&mut cmd);
        push_shell_args(&mut cmd, program, args);
        cmd.current_dir(working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_process_tree(&mut cmd);
        child_env::apply_to_command(&mut cmd, child_env::string_map_env(&exec_env.env));

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn managed command: {original_command}"))?;
        #[cfg(windows)]
        let windows_job = attach_windows_job(&child, original_command);
        let stdout_handle = child.stdout.take().context("Failed to capture stdout")?;
        let stderr_handle = child.stderr.take().context("Failed to capture stderr")?;
        let stdout_buffer = Arc::new(Mutex::new(Vec::new()));
        let stderr_buffer = Arc::new(Mutex::new(Vec::new()));
        let stdout_thread = Some(spawn_reader_thread(
            stdout_handle,
            Arc::clone(&stdout_buffer),
        ));
        let stderr_thread = Some(spawn_reader_thread(
            stderr_handle,
            Arc::clone(&stderr_buffer),
        ));

        let process = ManagedProcess {
            status: ShellStatus::Running,
            exit_code: None,
            started_at: started,
            sandbox_type,
            stdout_buffer,
            stderr_buffer,
            child: Some(child),
            #[cfg(windows)]
            windows_job,
            stdout_thread,
            stderr_thread,
        };
        self.processes.insert(process_id.clone(), process);
        Ok(process_id)
    }

    fn poll(&mut self, process_id: &str) -> Result<ShellResult> {
        let process = self
            .processes
            .get_mut(process_id)
            .ok_or_else(|| anyhow!("Managed process {process_id} not found"))?;
        let finished = process.poll();
        let snapshot = process.snapshot();
        if finished {
            self.processes.remove(process_id);
        }
        Ok(snapshot)
    }

    fn kill(&mut self, process_id: &str) -> Result<ShellResult> {
        let mut process = self
            .processes
            .remove(process_id)
            .ok_or_else(|| anyhow!("Managed process {process_id} not found"))?;
        process.kill()?;
        Ok(process.snapshot())
    }
}

/// Thread-safe wrapper for `ShellManager`
pub(crate) type SharedShellManager = Arc<Mutex<ShellManager>>;

/// Create a new shared shell manager with default sandbox policy.
pub(crate) fn new_shared_shell_manager(workspace: PathBuf) -> SharedShellManager {
    Arc::new(Mutex::new(ShellManager::new(workspace)))
}

#[cfg(test)]
mod tests;
