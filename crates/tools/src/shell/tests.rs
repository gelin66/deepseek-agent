use std::process::Command;
use std::time::{Duration, Instant};

use tempfile::tempdir;

use super::*;
use crate::sandbox::CommandSpec;

fn echo_command(message: &str) -> String {
    format!("echo {message}")
}

fn sleep_command(seconds: u64) -> String {
    let dispatcher = crate::shell_dispatcher::global_dispatcher();
    if dispatcher.kind().is_powershell() {
        return format!("Start-Sleep -Seconds {seconds}");
    }
    #[cfg(windows)]
    {
        let ping_count = seconds.saturating_add(1);
        format!("ping 127.0.0.1 -n {ping_count} > NUL")
    }
    #[cfg(not(windows))]
    {
        format!("sleep {seconds}")
    }
}

#[test]
fn running_job_snapshot_marks_no_output_stale_after_threshold() {
    let workspace = tempdir().expect("workspace");
    let mut manager = ShellManager::new(workspace.path().to_path_buf());
    let started = manager
        .execute(&sleep_command(5), None, 5_000, true)
        .expect("execute");
    let task_id = started.task_id.expect("task id");

    manager
        .processes
        .get_mut(&task_id)
        .expect("live shell")
        .last_output_at = Instant::now() - STALE_NO_OUTPUT_AFTER - Duration::from_millis(1);

    let job = manager
        .list_jobs()
        .into_iter()
        .find(|job| job.id == task_id)
        .expect("running job");
    assert_eq!(job.status, ShellStatus::Running);
    assert!(job.stale, "silent running job should be marked stale");
    assert!(
        job.elapsed_since_output_ms
            .is_some_and(|elapsed| elapsed >= STALE_NO_OUTPUT_AFTER.as_millis() as u64),
        "elapsed no-output time should be exposed: {job:?}"
    );
    manager.kill(&task_id).expect("cleanup");
}

#[test]
fn completed_background_shell_releases_process_handles() {
    let workspace = tempdir().expect("workspace");
    let mut manager = ShellManager::new(workspace.path().to_path_buf());
    let started = manager
        .execute(&echo_command("done"), None, 5_000, true)
        .expect("execute");
    let task_id = started.task_id.expect("task id");
    let result = manager
        .get_output(&task_id, true, 5_000)
        .expect("wait for completion");
    assert_eq!(result.status, ShellStatus::Completed);

    let shell = manager.processes.get_mut(&task_id).expect("tracked shell");
    shell.poll();
    assert_eq!(shell.status, ShellStatus::Completed);
    assert!(shell.stdin.is_none());
    assert!(shell.child.is_none());
    assert!(shell.stdout_thread.is_none());
    assert!(shell.stderr_thread.is_none());
}

#[test]
fn cleanup_removes_completed_process_owners() {
    let workspace = tempdir().expect("workspace");
    let mut manager = ShellManager::new(workspace.path().to_path_buf());
    let started = manager
        .execute(&echo_command("done"), None, 5_000, true)
        .expect("execute");
    let task_id = started.task_id.expect("task id");
    manager
        .get_output(&task_id, true, 3_000)
        .expect("completed output");
    assert!(!manager.processes.is_empty());
    manager.cleanup(Duration::ZERO);
    assert!(manager.processes.is_empty());
}

#[test]
fn issue_1691_quoted_commit_message_round_trips() {
    let command = r#"git commit -m "feat: complete sub-pages""#;
    let spec = CommandSpec::shell(
        command,
        std::path::PathBuf::from("/tmp"),
        Duration::from_secs(5),
    );
    let dispatcher = crate::shell_dispatcher::global_dispatcher();
    assert_eq!(spec.program, dispatcher.kind().binary());
    if dispatcher.kind().is_powershell() {
        assert_eq!(
            spec.args,
            [
                dispatcher.kind().command_flag().to_string(),
                "-Command".to_string(),
                format!("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {command}")
            ]
        );
    } else if matches!(dispatcher.kind(), crate::shell_dispatcher::ShellKind::Cmd) {
        assert_eq!(
            spec.args,
            ["/C".to_string(), format!("chcp 65001 >NUL & {command}")]
        );
    } else {
        assert_eq!(
            spec.args,
            [
                dispatcher.kind().command_flag().to_string(),
                command.to_string()
            ]
        );
    }

    let mut built = Command::new(&spec.program);
    push_shell_args(&mut built, &spec.program, &spec.args);
    let got = built
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(got, spec.args);
}

#[cfg(windows)]
mod windows_tests {
    use std::io::Read;
    use std::process::Stdio;

    use wait_timeout::ChildExt;
    use windows::Win32::Foundation::{DUPLICATE_HANDLE_OPTIONS, DuplicateHandle, HANDLE};
    use windows::Win32::System::Threading::GetCurrentProcess;

    use super::*;

    const JOB_OBJECT_QUERY_ACCESS: u32 = 0x0004;

    fn duplicate_job_without_terminate_access(job: WindowsJob) -> WindowsJob {
        let process = unsafe { GetCurrentProcess() };
        let mut limited_handle = HANDLE::default();
        unsafe {
            DuplicateHandle(
                process,
                job.handle,
                process,
                &mut limited_handle,
                JOB_OBJECT_QUERY_ACCESS,
                false,
                DUPLICATE_HANDLE_OPTIONS(0),
            )
            .expect("duplicate job handle without terminate access");
        }
        drop(job);
        WindowsJob {
            handle: limited_handle,
        }
    }

    #[test]
    fn terminate_denied_falls_back_to_child_kill() {
        let mut child = Command::new("ping")
            .args(["127.0.0.1", "-n", "20"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ping");
        let job = WindowsJob::attach_to_child(&child).expect("attach job");
        let limited_job = duplicate_job_without_terminate_access(job);
        assert!(limited_job.terminate().is_err());
        terminate_child_and_close_windows_job(Some(limited_job), &mut child)
            .expect("fallback child kill");
        assert!(
            child
                .wait_timeout(Duration::from_secs(3))
                .expect("wait after fallback kill")
                .is_some()
        );
    }

    #[test]
    fn job_close_releases_foreground_reader_threads_when_terminate_denied() {
        let mut child = Command::new("ping")
            .args(["127.0.0.1", "-n", "8"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn ping");
        let job = WindowsJob::attach_to_child(&child).expect("attach job");
        let limited_job = duplicate_job_without_terminate_access(job);
        assert!(limited_job.terminate().is_err());

        let stdout_handle = child.stdout.take().expect("stdout pipe");
        let stderr_handle = child.stderr.take().expect("stderr pipe");
        let stdout_thread = std::thread::spawn(move || {
            let mut reader = stdout_handle;
            let mut buffer = Vec::new();
            let _ = reader.read_to_end(&mut buffer);
            buffer
        });
        let stderr_thread = std::thread::spawn(move || {
            let mut reader = stderr_handle;
            let mut buffer = Vec::new();
            let _ = reader.read_to_end(&mut buffer);
            buffer
        });

        let started = Instant::now();
        terminate_and_close_windows_job(Some(limited_job));
        let _ = stdout_thread.join().unwrap_or_default();
        let _ = stderr_thread.join().unwrap_or_default();
        let status = child
            .wait_timeout(Duration::from_secs(3))
            .expect("wait after kill-on-close");
        assert!(started.elapsed() < Duration::from_secs(4));
        assert!(status.is_some());
    }

    #[test]
    fn kill_on_close_releases_background_reader_threads_when_terminate_denied() {
        let workspace = tempdir().expect("workspace");
        let mut manager = ShellManager::new(workspace.path().to_path_buf());
        let result = manager
            .execute(
                r#"cmd /c start "" /b ping 127.0.0.1 -n 8"#,
                None,
                5_000,
                true,
            )
            .expect("execute");
        let task_id = result.task_id.expect("task id");
        {
            let shell = manager
                .processes
                .get_mut(&task_id)
                .expect("background shell");
            let job = shell.windows_job.take().expect("windows job attached");
            let limited_job = duplicate_job_without_terminate_access(job);
            assert!(limited_job.terminate().is_err());
            shell.windows_job = Some(limited_job);
        }
        let started = Instant::now();
        let done = manager
            .get_output(&task_id, true, 3_000)
            .expect("get_output");
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(done.status, ShellStatus::Completed);
    }
}
