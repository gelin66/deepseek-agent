use std::path::Path;

/// Atomically write `contents` to `path` using a same-directory temporary
/// file, data sync, rename, and best-effort parent-directory sync.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no parent directory: {}", path.display()),
        )
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    std::io::Write::write_all(&mut temporary, contents)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    if let Ok(directory) = std::fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
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
}
