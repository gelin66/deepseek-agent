use std::fs;
use std::io;
use std::path::Path;

/// Open a workspace text candidate without following a final symlink.
///
/// Callers still validate file type, size, encoding, and workspace-relative
/// identity for their own contract.
#[cfg(unix)]
pub(crate) fn open_no_follow(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
pub(crate) fn open_no_follow(path: &Path) -> io::Result<fs::File> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "workspace text candidate is a symlink",
        ));
    }
    fs::File::open(path)
}
