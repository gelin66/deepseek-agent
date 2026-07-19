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
fn managed_foreground_releases_process_owner_after_completion() {
    let workspace = tempdir().expect("workspace");
    let mut manager = ShellManager::new(workspace.path().to_path_buf());
    let started = manager
        .spawn_shell(&echo_command("done"), None, 5_000, None, HashMap::new())
        .expect("spawn");
    let process_id = started;
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = loop {
        let snapshot = manager.poll(&process_id).expect("poll");
        if snapshot.status != ShellStatus::Running {
            break snapshot;
        }
        assert!(Instant::now() < deadline, "managed process did not finish");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(result.status, ShellStatus::Completed);
    assert!(manager.processes.is_empty());
}

#[test]
fn killing_managed_foreground_releases_process_owner() {
    let workspace = tempdir().expect("workspace");
    let mut manager = ShellManager::new(workspace.path().to_path_buf());
    let started = manager
        .spawn_shell(&sleep_command(5), None, 5_000, None, HashMap::new())
        .expect("spawn");
    let process_id = started;
    let killed = manager.kill(&process_id).expect("kill");
    assert_eq!(killed.status, ShellStatus::Killed);
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
    fn kill_on_close_releases_managed_reader_threads_when_terminate_denied() {
        let workspace = tempdir().expect("workspace");
        let mut manager = ShellManager::new(workspace.path().to_path_buf());
        let result = manager
            .spawn_shell(
                r#"cmd /c start "" /b ping 127.0.0.1 -n 8"#,
                None,
                5_000,
                None,
                HashMap::new(),
            )
            .expect("spawn");
        let process_id = result;
        {
            let process = manager
                .processes
                .get_mut(&process_id)
                .expect("managed process");
            let job = process.windows_job.take().expect("windows job attached");
            let limited_job = duplicate_job_without_terminate_access(job);
            assert!(limited_job.terminate().is_err());
            process.windows_job = Some(limited_job);
        }
        let started = Instant::now();
        let done = loop {
            let snapshot = manager.poll(&process_id).expect("poll");
            if snapshot.status != ShellStatus::Running {
                break snapshot;
            }
            assert!(started.elapsed() < Duration::from_secs(4));
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(done.status, ShellStatus::Completed);
        assert!(manager.processes.is_empty());
    }
}
