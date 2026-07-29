//! Bubblewrap (bwrap) passthrough for Linux sandbox (#2184).
//!
//! Bubblewrap is a setuid-less container runtime used by Flatpak and other
//! projects. It creates a new mount namespace with configurable bind mounts,
//! providing filesystem isolation without requiring root privileges.
//!
//! # How it works
//!
//! When `/usr/bin/bwrap` is present AND the config key `[sandbox] prefer_bwrap`
//! is set to `true`, exec_shell commands are routed through bwrap instead of
//! relying solely on Landlock. The bwrap invocation looks like:
//!
//! ```text
//! bwrap \
//!   --ro-bind / / \
//!   --bind <cwd> <cwd> \
//!   --chdir <cwd> \
//!   --unshare-all \
//!   -- <program> <args>
//! ```
//!
//! This creates a read-only view of the entire filesystem with write access
//! limited to the working directory.
//!
//! # Important
//!
//! We do NOT vendor bwrap. The user must install it themselves:
//!
//! - Ubuntu/Debian: `apt install bubblewrap`
//! - Fedora: `dnf install bubblewrap`
//! - Arch: `pacman -S bubblewrap`
//!
//! If bwrap is not installed, we fall back to Landlock.

#[cfg(any(target_os = "linux", test))]
use std::io;

/// Canonical path to the bubblewrap binary.
pub const BWRAP_PATH: &str = "/usr/bin/bwrap";

#[cfg(any(target_os = "linux", test))]
const ISOLATED_WRITER_PROTECTED_DIRECTORIES: [&str; 2] = [".dse", ".deepseek"];

/// Check if bubblewrap is installed and executable.
#[cfg(target_os = "linux")]
pub fn is_available() -> bool {
    std::path::Path::new(BWRAP_PATH).exists()
}

#[cfg(not(target_os = "linux"))]
pub fn is_available() -> bool {
    false
}

/// Materialize the protected directory mount points required by an isolated
/// writer before bubblewrap bind-mounts the writable worktree.
///
/// Bubblewrap cannot make a nonexistent child of a writable bind mount
/// read-only. Empty Host-created directories are invisible to Git, but give
/// bwrap stable mount points that the child cannot replace or populate.
/// Existing symlinks and non-directory entries fail closed instead of being
/// followed outside the writer worktree.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn prepare_isolated_writer_protected_paths(
    policy: &crate::sandbox::SandboxPolicy,
) -> io::Result<()> {
    let crate::sandbox::SandboxPolicy::IsolatedWriter { workspace, .. } = policy else {
        return Ok(());
    };
    let workspace = workspace.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot canonicalize isolated writer workspace {}: {error}",
                workspace.display()
            ),
        )
    })?;

    let git_path = workspace.join(".git");
    let git_metadata = std::fs::symlink_metadata(&git_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "isolated writer Git metadata path {} is unavailable: {error}",
                git_path.display()
            ),
        )
    })?;
    if git_metadata.file_type().is_symlink() || !(git_metadata.is_file() || git_metadata.is_dir()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "isolated writer Git metadata path {} must be a regular file or directory, not a symlink or special entry",
                git_path.display()
            ),
        ));
    }

    let mut missing = Vec::new();
    for name in ISOLATED_WRITER_PROTECTED_DIRECTORIES {
        let path = workspace.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "isolated writer protected path {} must be a real directory",
                        path.display()
                    ),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => missing.push(path),
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "cannot inspect isolated writer protected path {}: {error}",
                        path.display()
                    ),
                ));
            }
        }
    }
    for path in missing {
        match std::fs::create_dir(&path) {
            Ok(()) => {}
            Err(create_error) if create_error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(create_error) => {
                return Err(io::Error::new(
                    create_error.kind(),
                    format!(
                        "cannot create isolated writer protected directory {}: {create_error}",
                        path.display()
                    ),
                ));
            }
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|inspect_error| {
            io::Error::new(
                inspect_error.kind(),
                format!(
                    "cannot verify isolated writer protected directory {}: {inspect_error}",
                    path.display()
                ),
            )
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "isolated writer protected path {} was replaced while being prepared",
                    path.display()
                ),
            ));
        }
    }

    Ok(())
}

/// Build a bwrap command that wraps the given program and arguments.
///
/// The returned command vector is suitable for use as `ExecEnv.command` —
/// it replaces the normal program+args with a bwrap invocation that sets
/// up a read-only root filesystem with write access only to the specified
/// working directory.
///
/// # Arguments
///
/// - `cwd` — working directory that gets writable bind-mount
/// - `program` — the program to run inside the container
/// - `args` — arguments to pass to the program
///
/// # Returns
///
/// A `Vec<String>` representing the full bwrap invocation.
pub fn build_bwrap_command(cwd: &std::path::Path, program: &str, args: &[String]) -> Vec<String> {
    let mut cmd: Vec<String> = Vec::with_capacity(10 + args.len());
    cmd.push(BWRAP_PATH.to_string());
    cmd.push("--ro-bind".to_string());
    cmd.push("/".to_string());
    cmd.push("/".to_string());

    let cwd_str = cwd.to_string_lossy().to_string();
    cmd.push("--bind".to_string());
    cmd.push(cwd_str.clone());
    cmd.push(cwd_str.clone());
    cmd.push("--chdir".to_string());
    cmd.push(cwd_str);
    cmd.push("--unshare-all".to_string());
    cmd.push("--".to_string());
    cmd.push(program.to_string());
    cmd.extend(args.iter().cloned());
    cmd
}

/// Build a bubblewrap command from the canonical writable-root projection.
pub fn build_bwrap_command_for_policy(
    policy: &crate::sandbox::SandboxPolicy,
    cwd: &std::path::Path,
    program: &str,
    args: &[String],
) -> Vec<String> {
    let mut cmd: Vec<String> = Vec::with_capacity(10 + args.len());

    cmd.push(BWRAP_PATH.to_string());

    // Read-only bind-mount the entire root filesystem.
    cmd.push("--ro-bind".to_string());
    cmd.push("/".to_string());
    cmd.push("/".to_string());

    // Add only policy-projected writable roots. Overlay protected children
    // read-only after the parent bind so `.git` remains immutable.
    for root in policy.get_writable_roots(cwd) {
        let root_path = root.root.to_string_lossy().to_string();
        cmd.push("--bind".to_string());
        cmd.push(root_path.clone());
        cmd.push(root_path);
        for read_only in root.read_only_subpaths {
            let path = read_only.to_string_lossy().to_string();
            cmd.push("--ro-bind".to_string());
            cmd.push(path.clone());
            cmd.push(path);
        }
    }

    // Change to the working directory inside the container.
    let cwd_str = cwd.to_string_lossy().to_string();
    cmd.push("--chdir".to_string());
    cmd.push(cwd_str);

    // Unshare all namespaces for maximum isolation.
    cmd.push("--unshare-all".to_string());

    // Separator between bwrap args and the command to run.
    cmd.push("--".to_string());

    // The actual program and its arguments.
    cmd.push(program.to_string());
    cmd.extend(args.iter().cloned());

    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn isolated_writer_fixture() -> tempfile::TempDir {
        let workspace = tempfile::tempdir().expect("writer workspace");
        std::fs::write(
            workspace.path().join(".git"),
            "gitdir: /tmp/dse-test-gitdir\n",
        )
        .expect("write Git pointer");
        workspace
    }

    fn assert_read_only_bind(command: &[String], path: &Path) {
        let path = path.to_string_lossy();
        assert!(
            command
                .windows(3)
                .any(|args| args == ["--ro-bind", path.as_ref(), path.as_ref()]),
            "missing read-only bind for {path}: {command:?}"
        );
    }

    #[test]
    fn test_is_available_does_not_panic() {
        let _ = is_available();
    }

    #[test]
    fn isolated_writer_materializes_missing_protected_mount_points() {
        let workspace = isolated_writer_fixture();
        let policy = crate::sandbox::SandboxPolicy::isolated_writer(workspace.path());
        assert!(!workspace.path().join(".dse").exists());
        assert!(!workspace.path().join(".deepseek").exists());

        prepare_isolated_writer_protected_paths(&policy).expect("prepare protected paths");
        for name in ISOLATED_WRITER_PROTECTED_DIRECTORIES {
            let protected = workspace.path().join(name);
            assert!(
                protected.is_dir(),
                "{} was not created",
                protected.display()
            );
            assert_eq!(
                std::fs::read_dir(&protected)
                    .expect("read protected directory")
                    .count(),
                0
            );
        }
    }

    #[test]
    fn isolated_writer_reuses_existing_real_protected_directories() {
        let workspace = isolated_writer_fixture();
        for name in ISOLATED_WRITER_PROTECTED_DIRECTORIES {
            let protected = workspace.path().join(name);
            std::fs::create_dir(&protected).expect("create protected directory");
            std::fs::write(protected.join("owned"), name).expect("write protected marker");
        }
        let policy = crate::sandbox::SandboxPolicy::isolated_writer(workspace.path());

        prepare_isolated_writer_protected_paths(&policy).expect("reuse protected paths");
        for name in ISOLATED_WRITER_PROTECTED_DIRECTORIES {
            assert_eq!(
                std::fs::read_to_string(workspace.path().join(name).join("owned"))
                    .expect("read protected marker"),
                name
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn isolated_writer_rejects_protected_path_symlinks_without_touching_targets() {
        use std::os::unix::fs::symlink;

        for name in ISOLATED_WRITER_PROTECTED_DIRECTORIES {
            let workspace = isolated_writer_fixture();
            let external = tempfile::tempdir().expect("external target");
            let marker = external.path().join("marker");
            std::fs::write(&marker, "unchanged").expect("write external marker");
            symlink(external.path(), workspace.path().join(name))
                .expect("create malicious protected symlink");
            let policy = crate::sandbox::SandboxPolicy::isolated_writer(workspace.path());

            let error = prepare_isolated_writer_protected_paths(&policy)
                .expect_err("protected symlink must fail closed");
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            assert_eq!(
                std::fs::read_to_string(&marker).expect("read external marker"),
                "unchanged"
            );
            assert!(
                std::fs::symlink_metadata(workspace.path().join(name))
                    .expect("inspect malicious symlink")
                    .file_type()
                    .is_symlink()
            );
            for other in ISOLATED_WRITER_PROTECTED_DIRECTORIES {
                if other != name {
                    assert!(
                        !workspace.path().join(other).exists(),
                        "fail-closed preflight must not partially prepare {other}"
                    );
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn isolated_writer_rejects_a_symlinked_git_entry() {
        use std::os::unix::fs::symlink;

        let workspace = tempfile::tempdir().expect("writer workspace");
        let external = tempfile::tempdir().expect("external Git target");
        symlink(external.path(), workspace.path().join(".git"))
            .expect("create malicious Git symlink");
        let policy = crate::sandbox::SandboxPolicy::isolated_writer(workspace.path());

        let error = prepare_isolated_writer_protected_paths(&policy)
            .expect_err("Git symlink must fail closed");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(!workspace.path().join(".dse").exists());
        assert!(!workspace.path().join(".deepseek").exists());
    }

    #[test]
    fn test_build_bwrap_command_structure() {
        let cwd = std::path::Path::new("/home/user/project");
        let cmd = build_bwrap_command(cwd, "sh", &["-c".to_string(), "echo hi".to_string()]);

        // Should start with bwrap
        assert_eq!(cmd[0], "/usr/bin/bwrap");

        // Should have ro-bind for root
        assert!(cmd.contains(&"--ro-bind".to_string()));

        // Should have --chdir
        assert!(cmd.contains(&"--chdir".to_string()));

        // Should end with the command
        assert_eq!(cmd[cmd.len() - 1], "echo hi");
        assert_eq!(cmd[cmd.len() - 2], "-c");
        assert_eq!(cmd[cmd.len() - 3], "sh");
    }

    #[test]
    fn isolated_writer_bwrap_argv_overlays_every_protected_path() {
        let workspace = isolated_writer_fixture();
        let policy = crate::sandbox::SandboxPolicy::isolated_writer(workspace.path());
        prepare_isolated_writer_protected_paths(&policy).expect("prepare protected paths");
        let command = build_bwrap_command_for_policy(
            &policy,
            workspace.path(),
            "/bin/sh",
            &["-c".to_owned(), "true".to_owned()],
        );
        let canonical_workspace = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");

        for name in [".git", ".dse", ".deepseek"] {
            assert_read_only_bind(&command, &canonical_workspace.join(name));
        }
    }

    #[test]
    fn host_loopback_probe_does_not_share_the_linux_network_namespace() {
        let workspace = isolated_writer_fixture();
        let policy = crate::sandbox::SandboxPolicy::isolated_writer(workspace.path())
            .for_host_loopback_probe()
            .expect("Host probe policy");
        let command = build_bwrap_command_for_policy(&policy, workspace.path(), "/bin/true", &[]);

        assert!(policy.has_host_loopback_access());
        assert!(command.iter().any(|argument| argument == "--unshare-all"));
        assert!(!command.iter().any(|argument| argument == "--share-net"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn available_bwrap_enforces_missing_and_existing_protected_paths() {
        use std::process::Command;

        if !is_available()
            || !Command::new(BWRAP_PATH)
                .args(["--ro-bind", "/", "/", "--", "/bin/true"])
                .status()
                .is_ok_and(|status| status.success())
        {
            return;
        }
        let workspace = isolated_writer_fixture();
        std::fs::create_dir(workspace.path().join(".deepseek"))
            .expect("create existing protected directory");
        std::fs::write(workspace.path().join(".deepseek/owned"), "unchanged")
            .expect("write existing protected file");
        let policy = crate::sandbox::SandboxPolicy::isolated_writer(workspace.path());
        prepare_isolated_writer_protected_paths(&policy).expect("prepare protected paths");

        let run = |script: &str| {
            let command = build_bwrap_command_for_policy(
                &policy,
                workspace.path(),
                "/bin/sh",
                &["-c".to_owned(), script.to_owned()],
            );
            Command::new(&command[0])
                .args(&command[1..])
                .current_dir(workspace.path())
                .output()
                .expect("run bubblewrap")
        };

        assert!(run("touch ordinary").status.success());
        for script in [
            "touch .dse/blocked",
            "touch .deepseek/blocked",
            "printf blocked >> .git",
        ] {
            assert!(
                !run(script).status.success(),
                "protected write unexpectedly succeeded: {script}"
            );
        }
        assert!(workspace.path().join("ordinary").is_file());
        assert!(!workspace.path().join(".dse/blocked").exists());
        assert!(!workspace.path().join(".deepseek/blocked").exists());
        assert_eq!(
            std::fs::read_to_string(workspace.path().join(".deepseek/owned"))
                .expect("read protected file"),
            "unchanged"
        );
    }
}
