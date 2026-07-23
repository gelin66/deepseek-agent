use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub(crate) enum AtomicWriteError {
    #[error("target changed before atomic publish")]
    Conflict,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Atomically write `contents` to `path` using a same-directory temporary
/// file, data sync, rename, and best-effort parent-directory sync.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    write_atomic_inner(path, contents, None, None).map_err(|error| match error {
        AtomicWriteError::Conflict => std::io::Error::other(error),
        AtomicWriteError::Io(error) => error,
    })
}

pub(crate) fn write_atomic_if_unchanged(
    path: &Path,
    expected: Option<&[u8]>,
    contents: &[u8],
    permissions: Option<&std::fs::Permissions>,
) -> Result<(), AtomicWriteError> {
    write_atomic_inner(path, contents, Some(expected), permissions)
}

fn write_atomic_inner(
    path: &Path,
    contents: &[u8],
    expected: Option<Option<&[u8]>>,
    permissions: Option<&std::fs::Permissions>,
) -> Result<(), AtomicWriteError> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no parent directory: {}", path.display()),
        )
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    std::io::Write::write_all(&mut temporary, contents)?;
    let preserved_permissions = permissions.cloned().or_else(|| {
        std::fs::metadata(path)
            .ok()
            .map(|metadata| metadata.permissions())
    });
    if let Some(permissions) = preserved_permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary.as_file().sync_all()?;
    #[cfg(test)]
    maybe_stop_for_crash_fixture("before_persist");

    match expected {
        Some(Some(expected)) => {
            match std::fs::read(path) {
                Ok(current) if current == expected => {}
                _ => return Err(AtomicWriteError::Conflict),
            }
            // This is an optimistic exact-byte precondition followed by an
            // atomic replacement. It is not a filesystem compare-and-swap:
            // an uncooperative external writer can still race this rename.
            temporary.persist(path).map_err(|error| error.error)?;
        }
        Some(None) => {
            // Unlike an existence check followed by rename, noclobber keeps
            // concurrent file creation from being overwritten.
            temporary.persist_noclobber(path).map_err(|error| {
                if error.error.kind() == std::io::ErrorKind::AlreadyExists {
                    AtomicWriteError::Conflict
                } else {
                    AtomicWriteError::Io(error.error)
                }
            })?;
        }
        None => {
            temporary.persist(path).map_err(|error| error.error)?;
        }
    }
    #[cfg(test)]
    maybe_stop_for_crash_fixture("after_persist");
    if let Ok(directory) = std::fs::File::open(parent) {
        directory.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
fn maybe_stop_for_crash_fixture(stage: &str) {
    if std::env::var("CODEWHALE_M7C_ATOMIC_STOP_STAGE").as_deref() != Ok(stage) {
        return;
    }
    let marker =
        std::env::var_os("CODEWHALE_M7C_ATOMIC_MARKER").expect("M7-C atomic crash marker path");
    std::fs::write(marker, stage).expect("publish M7-C atomic crash marker");
    loop {
        std::thread::park_timeout(std::time::Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn writes_exact_bytes() {
        let workspace = tempdir().expect("workspace");
        let path = workspace.path().join("bytes.bin");
        let bytes = b"hello\0atomic\r\nworld";

        write_atomic(&path, bytes).expect("atomic write");

        assert_eq!(fs::read(&path).expect("read"), bytes);
    }

    #[test]
    fn replaces_existing_file_without_publishing_a_temp_file() {
        let workspace = tempdir().expect("workspace");
        let path = workspace.path().join("existing.txt");
        fs::write(&path, b"old content").expect("old content");

        write_atomic(&path, b"new content").expect("atomic replacement");

        assert_eq!(fs::read(&path).expect("read"), b"new content");
        let entries: Vec<_> = fs::read_dir(workspace.path())
            .expect("read directory")
            .collect::<Result<_, _>>()
            .expect("directory entries");
        assert_eq!(entries.len(), 1, "temporary file must not remain");
        assert_eq!(entries[0].path(), path);
    }

    #[cfg(unix)]
    #[test]
    fn replaces_existing_file_without_dropping_mode() {
        use std::os::unix::fs::PermissionsExt;

        let workspace = tempdir().expect("workspace");
        let path = workspace.path().join("executable.sh");
        fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("fixture mode");

        write_atomic(&path, b"#!/bin/sh\nexit 1\n").expect("atomic replacement");

        assert_eq!(
            fs::metadata(&path).expect("metadata").permissions().mode() & 0o777,
            0o755
        );
    }

    #[test]
    fn checked_publish_rejects_stale_original() {
        let workspace = tempdir().expect("workspace");
        let path = workspace.path().join("value.txt");
        fs::write(&path, b"current").expect("fixture");

        let error = write_atomic_if_unchanged(&path, Some(b"stale"), b"new", None)
            .expect_err("stale bytes must fail closed");

        assert!(matches!(error, AtomicWriteError::Conflict));
        assert_eq!(fs::read(path).expect("unchanged"), b"current");
    }

    #[test]
    fn checked_create_never_clobbers_an_existing_target() {
        let workspace = tempdir().expect("workspace");
        let path = workspace.path().join("value.txt");
        fs::write(&path, b"external").expect("external fixture");

        let error = write_atomic_if_unchanged(&path, None, b"candidate", None)
            .expect_err("checked create must not replace an existing target");

        assert!(matches!(error, AtomicWriteError::Conflict));
        assert_eq!(fs::read(path).expect("external bytes survive"), b"external");
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "external crash child"]
    fn m7c_atomic_write_crash_child() {
        if std::env::var("CODEWHALE_M7C_ATOMIC_CHILD").as_deref() != Ok("1") {
            return;
        }
        let path = std::path::PathBuf::from(
            std::env::var_os("CODEWHALE_M7C_ATOMIC_TARGET").expect("atomic target"),
        );
        write_atomic(&path, b"new\n").expect("atomic child write");
    }

    #[cfg(unix)]
    #[test]
    fn m7c_atomic_replace_crash_boundaries_publish_only_old_or_new() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        for (stage, expected) in [("before_persist", b"old\n"), ("after_persist", b"new\n")] {
            let workspace = tempdir().expect("workspace");
            let target = workspace.path().join("value.txt");
            let marker = workspace.path().join("marker");
            fs::write(&target, b"old\n").expect("fixture");
            let mut child = Command::new(std::env::current_exe().expect("current test binary"))
                .args([
                    "--ignored",
                    "--exact",
                    "atomic_write::tests::m7c_atomic_write_crash_child",
                    "--nocapture",
                ])
                .env("CODEWHALE_M7C_ATOMIC_CHILD", "1")
                .env("CODEWHALE_M7C_ATOMIC_STOP_STAGE", stage)
                .env("CODEWHALE_M7C_ATOMIC_MARKER", &marker)
                .env("CODEWHALE_M7C_ATOMIC_TARGET", &target)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn atomic crash child");
            let deadline = Instant::now() + Duration::from_secs(5);
            while !marker.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(marker.exists(), "child did not reach {stage}");
            let kill_result = unsafe { libc::kill(child.id() as i32, libc::SIGKILL) };
            assert_eq!(kill_result, 0, "SIGKILL atomic crash child");
            let status = child.wait().expect("reap atomic crash child");
            assert_eq!(status.signal(), Some(libc::SIGKILL));
            assert_eq!(
                fs::read(&target).expect("target remains readable"),
                expected,
                "atomic replace must expose only old or new bytes at {stage}"
            );
        }
    }
}
