use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{ProductionToolContext, ToolError, ToolOutcome, optional_str};

const LIST_DIR_TIMEOUT: Duration = Duration::from_secs(30);

/// Cap one directory result so build output and dependency trees cannot
/// consume the model context. Small directories retain the plain array shape.
const LIST_DIR_MAX_ENTRIES: usize = 500;

/// Execute the production `list_dir` operation against a workspace context.
pub async fn execute_list_dir(
    input: Value,
    context: &ProductionToolContext,
) -> Result<ToolOutcome, ToolError> {
    let path = optional_str(&input, "path").unwrap_or(".");
    let directory = context.resolve_path(path)?;
    let entries = list_dir_entries_async(
        directory,
        context.cancellation_token().cloned(),
        LIST_DIR_TIMEOUT,
    )
    .await?;

    ToolOutcome::json(&entries).map_err(|error| ToolError::execution_failed(error.to_string()))
}

async fn list_dir_entries_async(
    directory: PathBuf,
    cancellation: Option<CancellationToken>,
    timeout: Duration,
) -> Result<Value, ToolError> {
    let worker_cancellation = cancellation.clone();
    run_blocking_list_dir(timeout, cancellation, move || {
        list_dir_entries(&directory, worker_cancellation.as_ref())
    })
    .await
}

async fn run_blocking_list_dir<F>(
    timeout: Duration,
    cancellation: Option<CancellationToken>,
    list_directory: F,
) -> Result<Value, ToolError>
where
    F: FnOnce() -> Result<Value, ToolError> + Send + 'static,
{
    if cancellation
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        return Err(list_dir_cancelled());
    }

    let task = tokio::task::spawn_blocking(list_directory);
    let result = match cancellation {
        Some(token) => {
            tokio::select! {
                biased;
                () = token.cancelled() => return Err(list_dir_cancelled()),
                result = tokio::time::timeout(timeout, task) => result,
            }
        }
        None => tokio::time::timeout(timeout, task).await,
    };

    let joined = result.map_err(|_| list_dir_timeout(timeout))?;
    joined.map_err(|error| {
        ToolError::execution_failed(format!("list_dir worker failed before completion: {error}"))
    })?
}

fn list_dir_entries(
    directory: &Path,
    cancellation: Option<&CancellationToken>,
) -> Result<Value, ToolError> {
    check_list_dir_cancelled(cancellation)?;

    let mut entries = Vec::new();
    let mut total_entries = 0usize;

    for entry in fs::read_dir(directory).map_err(|error| {
        ToolError::execution_failed(format!(
            "Failed to read directory {}: {}",
            directory.display(),
            error
        ))
    })? {
        check_list_dir_cancelled(cancellation)?;

        let entry = entry.map_err(|error| ToolError::execution_failed(error.to_string()))?;
        total_entries += 1;
        if entries.len() >= LIST_DIR_MAX_ENTRIES {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| ToolError::execution_failed(error.to_string()))?;

        entries.push(json!({
            "name": entry.file_name().to_string_lossy().to_string(),
            "is_dir": file_type.is_dir(),
        }));
    }

    if total_entries > entries.len() {
        Ok(json!({
            "entries": entries,
            "listed_entries": LIST_DIR_MAX_ENTRIES,
            "total_entries": total_entries,
            "truncated": true,
        }))
    } else {
        Ok(Value::Array(entries))
    }
}

fn check_list_dir_cancelled(cancellation: Option<&CancellationToken>) -> Result<(), ToolError> {
    if cancellation.is_some_and(CancellationToken::is_cancelled) {
        return Err(list_dir_cancelled());
    }
    Ok(())
}

fn list_dir_cancelled() -> ToolError {
    ToolError::execution_failed("list_dir cancelled before completion")
}

fn list_dir_timeout(timeout: Duration) -> ToolError {
    ToolError::Timeout {
        seconds: timeout.as_secs().max(1),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn lists_the_workspace_root_by_default() {
        let workspace = tempdir().expect("tempdir");
        fs::write(workspace.path().join("file1.txt"), "").expect("write");
        fs::write(workspace.path().join("file2.txt"), "").expect("write");
        fs::create_dir(workspace.path().join("subdir")).expect("mkdir");
        let context = ProductionToolContext::new(workspace.path());

        let result = execute_list_dir(json!({}), &context)
            .await
            .expect("execute");

        assert!(result.is_success());
        let entries: Value = serde_json::from_str(&result.content).expect("list_dir json");
        let entries = entries.as_array().expect("plain entry array");
        assert!(
            entries
                .iter()
                .any(|entry| { entry.get("name").and_then(Value::as_str) == Some("file1.txt") })
        );
        assert!(
            entries
                .iter()
                .any(|entry| { entry.get("name").and_then(Value::as_str) == Some("file2.txt") })
        );
        assert!(entries.iter().any(|entry| {
            entry.get("name").and_then(Value::as_str) == Some("subdir")
                && entry.get("is_dir").and_then(Value::as_bool) == Some(true)
        }));
    }

    #[tokio::test]
    async fn resolves_a_relative_directory_path() {
        let workspace = tempdir().expect("tempdir");
        let nested = workspace.path().join("mydir");
        fs::create_dir(&nested).expect("mkdir");
        fs::write(nested.join("nested.txt"), "").expect("write");
        let context = ProductionToolContext::new(workspace.path());

        let result = execute_list_dir(json!({"path": "mydir"}), &context)
            .await
            .expect("execute");

        assert!(result.content.contains("nested.txt"));
    }

    #[tokio::test]
    async fn keeps_the_plain_array_shape_for_small_directories() {
        let workspace = tempdir().expect("tempdir");
        fs::write(workspace.path().join("only.txt"), "").expect("write");
        let context = ProductionToolContext::new(workspace.path());

        let result = execute_list_dir(json!({}), &context)
            .await
            .expect("execute");

        let parsed: Value = serde_json::from_str(&result.content).expect("json");
        assert!(
            parsed.is_array(),
            "small directories remain arrays: {parsed}"
        );
        assert_eq!(parsed.as_array().expect("entries").len(), 1);
    }

    #[tokio::test]
    async fn caps_large_directories_with_truncation_metadata() {
        let workspace = tempdir().expect("tempdir");
        let extra = 7;
        for index in 0..LIST_DIR_MAX_ENTRIES + extra {
            fs::write(workspace.path().join(format!("f{index:04}.txt")), "").expect("write");
        }
        let context = ProductionToolContext::new(workspace.path());

        let result = execute_list_dir(json!({}), &context)
            .await
            .expect("execute");

        let parsed: Value = serde_json::from_str(&result.content).expect("json");
        assert_eq!(parsed["truncated"], json!(true));
        assert_eq!(parsed["listed_entries"], json!(LIST_DIR_MAX_ENTRIES));
        assert_eq!(parsed["total_entries"], json!(LIST_DIR_MAX_ENTRIES + extra));
        assert_eq!(
            parsed["entries"].as_array().expect("entries").len(),
            LIST_DIR_MAX_ENTRIES
        );
    }

    #[tokio::test]
    async fn respects_an_already_cancelled_invocation() {
        let workspace = tempdir().expect("tempdir");
        fs::write(workspace.path().join("file.txt"), "").expect("write");
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let context = ProductionToolContext::new(workspace.path()).for_invocation(cancellation);

        let error = execute_list_dir(json!({}), &context)
            .await
            .expect_err("cancelled list_dir should fail");

        assert!(format!("{error:?}").contains("cancelled"));
    }

    #[tokio::test]
    async fn reports_worker_timeout_as_a_typed_tool_error() {
        let error = run_blocking_list_dir(Duration::from_millis(1), None, || {
            std::thread::sleep(Duration::from_millis(50));
            Ok(Value::Array(Vec::new()))
        })
        .await
        .expect_err("slow list_dir worker should time out");

        assert!(matches!(error, ToolError::Timeout { seconds: 1 }));
    }
}
