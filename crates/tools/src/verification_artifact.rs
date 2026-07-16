//! Revision-bound deterministic verification artifacts.
//!
//! These artifacts prove what command ran against which workspace revision.
//! They are evidence only: Goal contracts, completion receipts and host
//! acceptance deliberately remain outside this crate.

use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use codewhale_protocol::agent_runtime::{
    ToolArtifact, ToolArtifactStatus, ToolEvidence, ToolEvidenceStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wait_timeout::ChildExt;

use crate::ToolOutcome;
use crate::shell::{ProcessTreeOwner, configure_process_tree};

const ARTIFACT_METADATA_KEY: &str = "verification_artifact";
const ARTIFACT_REJECTION_METADATA_KEY: &str = "verification_artifact_rejected";
const ARTIFACT_ID_PREFIX: &str = "verification-evidence:";
const ARTIFACT_MEDIA_TYPE: &str = "application/vnd.codewhale.verification+json";
const MAX_GIT_DIFF_BYTES: usize = 256 * 1024 * 1024;
const MAX_GIT_PATH_LIST_BYTES: usize = 16 * 1024 * 1024;
const MAX_GIT_STDERR_BYTES: usize = 64 * 1024;
const MAX_UNTRACKED_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_UNTRACKED_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const GIT_READER_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationArtifact {
    pub tool: String,
    pub check: String,
    pub summary: String,
    pub workspace_revision: String,
    pub verifier_id: String,
    pub verifier_params_sha256: String,
}

/// Capture Git HEAD, staged/unstaged binary diffs and every non-ignored
/// untracked file in a bounded digest.
pub async fn capture_workspace_revision(workspace: &Path) -> Result<String, String> {
    let workspace = workspace.to_path_buf();
    tokio::task::spawn_blocking(move || capture_workspace_revision_sync(&workspace))
        .await
        .map_err(|error| format!("workspace revision task failed: {error}"))?
}

/// Attach evidence only when the checker succeeded and the workspace stayed
/// unchanged for its entire execution.
pub fn attach_verification_artifact(
    result: &mut ToolOutcome,
    tool: &str,
    verifier_params: &Value,
    check: String,
    summary: String,
    revision_before: Result<String, String>,
    revision_after: Result<String, String>,
) {
    let observation = match (revision_before, revision_after) {
        (Ok(before), Ok(after)) if before == after && result.is_success() => {
            Ok(VerificationArtifact {
                tool: tool.to_string(),
                check,
                summary,
                workspace_revision: after,
                verifier_id: tool.to_string(),
                verifier_params_sha256: structured_value_hash(
                    "verification-params-v1",
                    verifier_params,
                ),
            })
        }
        (Ok(_), Ok(_)) if result.is_success() => Err((
            ToolEvidenceStatus::Stale,
            "workspace changed while the evidence command was running; rerun it in the final workspace"
                .to_string(),
        )),
        (Err(error), _) | (_, Err(error)) if result.is_success() => Err((
            ToolEvidenceStatus::Missing,
            format!("could not bind checker evidence to the workspace revision: {error}"),
        )),
        _ => {
            reject_verification_artifact(result);
            return;
        }
    };

    let metadata_entry = match observation {
        Ok(artifact) => {
            let artifact_value =
                serde_json::to_value(&artifact).expect("verification artifact is serializable");
            let artifact_bytes = serde_json::to_vec(&canonical_json(&artifact_value))
                .expect("verification artifact JSON is serializable");
            let artifact_sha256 = format_sha256(Sha256::digest(&artifact_bytes).as_slice());
            let artifact_id = format!("{ARTIFACT_ID_PREFIX}{artifact_sha256}");
            result.evidence = ToolEvidence {
                status: ToolEvidenceStatus::Produced,
                references: vec![artifact_id.clone()],
            };
            result.artifacts = vec![ToolArtifact {
                id: artifact_id,
                status: ToolArtifactStatus::Available,
                sha256: Some(artifact_sha256),
                media_type: Some(ARTIFACT_MEDIA_TYPE.to_string()),
                byte_len: Some(
                    u64::try_from(artifact_bytes.len())
                        .expect("verification artifact length fits in u64"),
                ),
            }];
            result.workspace_revision = Some(artifact.workspace_revision.clone());
            (ARTIFACT_METADATA_KEY.to_string(), artifact_value)
        }
        Err((status, reason)) => {
            set_verification_status(result, status);
            (
                ARTIFACT_REJECTION_METADATA_KEY.to_string(),
                Value::String(reason),
            )
        }
    };
    let metadata = result.metadata.get_or_insert_with(|| json!({}));
    if !metadata.is_object() {
        *metadata = json!({"tool_metadata": metadata.take()});
    }
    metadata
        .as_object_mut()
        .expect("metadata was normalized to an object")
        .insert(metadata_entry.0, metadata_entry.1);
}

/// Mark a checker result as unusable evidence without manufacturing a
/// workspace binding or artifact reference.
pub fn reject_verification_artifact(result: &mut ToolOutcome) {
    set_verification_status(result, ToolEvidenceStatus::Rejected);
}

fn set_verification_status(result: &mut ToolOutcome, status: ToolEvidenceStatus) {
    result.evidence = ToolEvidence {
        status,
        references: Vec::new(),
    };
    result.artifacts.clear();
    result.workspace_revision = None;
}

fn capture_workspace_revision_sync(workspace: &Path) -> Result<String, String> {
    let canonical_workspace = workspace
        .canonicalize()
        .map_err(|error| format!("cannot resolve workspace {}: {error}", workspace.display()))?;
    let root_raw = run_git_capped(
        &canonical_workspace,
        &["rev-parse", "--show-toplevel"],
        MAX_GIT_PATH_LIST_BYTES,
    )?;
    let root_text = std::str::from_utf8(&root_raw)
        .map_err(|_| "git repository root is not valid UTF-8".to_string())?
        .trim();
    if root_text.is_empty() {
        return Err("git did not report a repository root".to_string());
    }
    let repository_root = PathBuf::from(root_text)
        .canonicalize()
        .map_err(|error| format!("cannot resolve Git repository root {root_text}: {error}"))?;
    let head = run_git_capped(
        &canonical_workspace,
        &["rev-parse", "--verify", "HEAD"],
        4 * 1024,
    )
    .unwrap_or_else(|_| b"unborn".to_vec());
    let staged = run_git_capped(
        &canonical_workspace,
        &[
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "--cached",
            "--",
            ".",
        ],
        MAX_GIT_DIFF_BYTES,
    )?;
    let unstaged = run_git_capped(
        &canonical_workspace,
        &[
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "--",
            ".",
        ],
        MAX_GIT_DIFF_BYTES,
    )?;
    let untracked = run_git_capped(
        &canonical_workspace,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "--full-name",
            "-z",
            "--",
            ".",
        ],
        MAX_GIT_PATH_LIST_BYTES,
    )?;

    let mut hasher = Sha256::new();
    hash_segment(
        &mut hasher,
        b"workspace",
        canonical_workspace.as_os_str().as_encoded_bytes(),
    );
    hash_segment(
        &mut hasher,
        b"repository",
        repository_root.as_os_str().as_encoded_bytes(),
    );
    hash_segment(&mut hasher, b"head", &head);
    hash_segment(&mut hasher, b"staged", &staged);
    hash_segment(&mut hasher, b"unstaged", &unstaged);

    let mut total_untracked_bytes = 0_u64;
    for raw_path in untracked
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = git_path_from_bytes(raw_path)?;
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        {
            return Err(format!(
                "git reported an unsafe untracked path: {}",
                relative.display()
            ));
        }
        let path = repository_root.join(&relative);
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "cannot inspect untracked path {} while sealing verifier evidence: {error}",
                path.display()
            )
        })?;
        hash_segment(&mut hasher, b"untracked-path", raw_path);
        if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&path)
                .map_err(|error| format!("cannot read symlink {}: {error}", path.display()))?;
            hash_segment(
                &mut hasher,
                b"untracked-symlink",
                target.as_os_str().as_encoded_bytes(),
            );
            continue;
        }
        if !metadata.is_file() {
            return Err(format!(
                "unsupported untracked filesystem entry {}",
                path.display()
            ));
        }
        if metadata.len() > MAX_UNTRACKED_FILE_BYTES {
            return Err(format!(
                "untracked file {} is too large to bind verifier evidence ({} bytes; limit {})",
                path.display(),
                metadata.len(),
                MAX_UNTRACKED_FILE_BYTES
            ));
        }
        total_untracked_bytes = total_untracked_bytes.saturating_add(metadata.len());
        if total_untracked_bytes > MAX_UNTRACKED_TOTAL_BYTES {
            return Err(format!(
                "untracked files are too large to bind verifier evidence (limit {MAX_UNTRACKED_TOTAL_BYTES} bytes)"
            ));
        }
        hash_file(&mut hasher, &path)?;
    }
    Ok(format_sha256(hasher.finalize().as_slice()))
}

fn hash_segment(hasher: &mut Sha256, label: &[u8], value: &[u8]) {
    hasher.update((label.len() as u64).to_le_bytes());
    hasher.update(label);
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn structured_value_hash(domain: &str, value: &Value) -> String {
    let canonical = canonical_json(value);
    let encoded = serde_json::to_vec(&canonical).expect("JSON value is serializable");
    let mut hasher = Sha256::new();
    hash_segment(&mut hasher, domain.as_bytes(), &encoded);
    format_sha256(hasher.finalize().as_slice())
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut canonical = serde_json::Map::new();
            for key in keys {
                canonical.insert(key.clone(), canonical_json(&object[key]));
            }
            Value::Object(canonical)
        }
        primitive => primitive.clone(),
    }
}

fn format_sha256(digest: &[u8]) -> String {
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

fn hash_file(hasher: &mut Sha256, path: &Path) -> Result<(), String> {
    let mut file =
        File::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    hasher.update(b"untracked-file\0");
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(())
}

#[cfg(unix)]
fn git_path_from_bytes(path: &[u8]) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(path.to_vec())))
}

#[cfg(not(unix))]
fn git_path_from_bytes(path: &[u8]) -> Result<PathBuf, String> {
    String::from_utf8(path.to_vec())
        .map(PathBuf::from)
        .map_err(|_| "git reported a non-UTF-8 untracked path".to_string())
}

fn run_git_capped(workspace: &Path, args: &[&str], cap: usize) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(workspace).args(args);
    run_command_capped(
        command,
        &format!("git {}", args.join(" ")),
        cap,
        GIT_COMMAND_TIMEOUT,
    )
}

fn run_command_capped(
    mut command: Command,
    label: &str,
    stdout_cap: usize,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    configure_process_tree(&mut command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run {label}: {error}"))?;
    let mut process_tree = ProcessTreeOwner::attach_std(&child, label).map_err(|error| {
        let _ = child.kill();
        let _ = child.wait();
        format!("cannot own {label} process tree: {error}")
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        let _ = process_tree.kill();
        let _ = child.kill();
        let _ = child.wait();
        format!("{label} stdout pipe was not created")
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        let _ = process_tree.kill();
        let _ = child.kill();
        let _ = child.wait();
        format!("{label} stderr pipe was not created")
    })?;
    let stdout_reader = spawn_capped_reader(stdout, stdout_cap);
    let stderr_reader = spawn_capped_reader(stderr, MAX_GIT_STDERR_BYTES);
    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = process_tree.kill();
            let _ = child.kill();
            let _ = child.wait();
            let _ = receive_capped_reader(stdout_reader, label, "stdout");
            let _ = receive_capped_reader(stderr_reader, label, "stderr");
            return Err(format!("{label} timed out while sealing verifier evidence"));
        }
        Err(error) => {
            let _ = process_tree.kill();
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("cannot wait for {label}: {error}"));
        }
    };
    let stdout = receive_capped_reader(stdout_reader, label, "stdout")?;
    let stderr = receive_capped_reader(stderr_reader, label, "stderr")?;
    let _ = process_tree.kill();
    process_tree.disarm();
    if !status.success() {
        return Err(format!(
            "{label} failed with {status}: {}",
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    Ok(stdout)
}

fn spawn_capped_reader(
    reader: impl Read + Send + 'static,
    cap: usize,
) -> std::sync::mpsc::Receiver<Result<Vec<u8>, String>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(read_capped(reader, cap));
    });
    receiver
}

fn receive_capped_reader(
    receiver: std::sync::mpsc::Receiver<Result<Vec<u8>, String>>,
    label: &str,
    stream: &str,
) -> Result<Vec<u8>, String> {
    match receiver.recv_timeout(GIT_READER_DRAIN_TIMEOUT) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(format!("{label} {stream} pipe did not close in time"))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(format!("{label} {stream} reader stopped unexpectedly"))
        }
    }
}

fn read_capped(mut reader: impl Read, cap: usize) -> Result<Vec<u8>, String> {
    let mut output = Vec::with_capacity(cap.min(64 * 1024));
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("cannot read git output: {error}"))?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > cap {
            return Err(format!("git output exceeded the {cap}-byte evidence limit"));
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn workspace_mutation_changes_revision() {
        let workspace = tempfile::tempdir().unwrap();
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(workspace.path())
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(workspace.path().join("fixture.txt"), "before").unwrap();
        let before = capture_workspace_revision(workspace.path()).await.unwrap();
        std::fs::write(workspace.path().join("fixture.txt"), "after").unwrap();
        let after = capture_workspace_revision(workspace.path()).await.unwrap();
        assert_ne!(before, after);
    }

    #[test]
    fn artifact_binds_exact_canonical_parameters() {
        let revision = Ok("sha256:revision".to_string());
        let mut first = ToolOutcome::success("ok");
        attach_verification_artifact(
            &mut first,
            "run_tests",
            &json!({"all_features": false, "args": ""}),
            "cargo test".to_string(),
            "passed".to_string(),
            revision.clone(),
            revision.clone(),
        );
        let first_hash =
            first.metadata.as_ref().unwrap()[ARTIFACT_METADATA_KEY]["verifier_params_sha256"]
                .as_str()
                .unwrap()
                .to_string();
        let mut second = ToolOutcome::success("ok");
        attach_verification_artifact(
            &mut second,
            "run_tests",
            &json!({"args": "--lib", "all_features": false}),
            "cargo test --lib".to_string(),
            "passed".to_string(),
            revision.clone(),
            revision,
        );
        let second_hash =
            second.metadata.as_ref().unwrap()[ARTIFACT_METADATA_KEY]["verifier_params_sha256"]
                .as_str()
                .unwrap();
        assert_ne!(first_hash, second_hash);
        assert_eq!(first.evidence.status, ToolEvidenceStatus::Produced);
        assert_eq!(first.artifacts.len(), 1);
        assert_eq!(first.workspace_revision.as_deref(), Some("sha256:revision"));
    }
}
