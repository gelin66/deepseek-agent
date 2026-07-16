use std::fs;

use serde_json::{Value, json};
use tempfile::tempdir;

use super::file::ListDirTool;
use super::file_search::FileSearchTool;
use super::git::{GitDiffTool, GitStatusTool};
use super::search::GrepFilesTool;
use super::spec::{ToolContext, ToolSpec};

#[test]
fn readonly_tool_schemas_match_the_pre_extraction_fixture() {
    let expected: Value = serde_json::from_str(include_str!("fixtures/readonly_tool_schemas.json"))
        .expect("valid schema fixture");

    for (name, schema) in [
        ("list_dir", ListDirTool.input_schema()),
        ("file_search", FileSearchTool.input_schema()),
        ("grep_files", GrepFilesTool.input_schema()),
        ("git_status", GitStatusTool.input_schema()),
        ("git_diff", GitDiffTool.input_schema()),
    ] {
        assert_eq!(schema, expected[name], "{name} schema drifted");
    }
}

#[tokio::test]
async fn tui_adapters_preserve_production_outputs_and_errors() {
    let workspace = tempdir().expect("tempdir");
    fs::write(workspace.path().join("needle.rs"), "fn needle() {}\n").expect("write");
    let context = ToolContext::new(workspace.path());

    let input = json!({});
    let expected = codewhale_tools::execute_list_dir(input.clone(), context.production_context())
        .await
        .expect("production list_dir");
    let actual = ListDirTool
        .execute(input, &context)
        .await
        .expect("adapter list_dir");
    assert_eq!(actual, expected);

    let input = json!({"query": "needle", "extensions": ["rs"]});
    let expected =
        codewhale_tools::execute_file_search(input.clone(), context.production_context())
            .await
            .expect("production file_search");
    let actual = FileSearchTool
        .execute(input, &context)
        .await
        .expect("adapter file_search");
    assert_eq!(actual, expected);

    let input = json!({"pattern": "needle", "context_lines": 0});
    let expected = codewhale_tools::execute_grep_files(input.clone(), context.production_context())
        .await
        .expect("production grep_files");
    let actual = GrepFilesTool
        .execute(input, &context)
        .await
        .expect("adapter grep_files");
    assert_eq!(actual, expected);

    let input = json!({"path": "missing"});
    let expected = codewhale_tools::execute_git_status(input.clone(), context.production_context())
        .expect_err("production git_status input error");
    let actual = GitStatusTool
        .execute(input, &context)
        .await
        .expect_err("adapter git_status input error");
    assert_eq!(actual.to_string(), expected.to_string());

    let input = json!({"path": "missing"});
    let expected = codewhale_tools::execute_git_diff(input.clone(), context.production_context())
        .expect_err("production git_diff input error");
    let actual = GitDiffTool
        .execute(input, &context)
        .await
        .expect_err("adapter git_diff input error");
    assert_eq!(actual.to_string(), expected.to_string());

    let input = json!({"query": "   "});
    let expected =
        codewhale_tools::execute_file_search(input.clone(), context.production_context())
            .await
            .expect_err("production file_search input error");
    let actual = FileSearchTool
        .execute(input, &context)
        .await
        .expect_err("adapter file_search input error");
    assert_eq!(actual.to_string(), expected.to_string());

    let input = json!({"pattern": "[invalid"});
    let expected = codewhale_tools::execute_grep_files(input.clone(), context.production_context())
        .await
        .expect_err("production grep_files input error");
    let actual = GrepFilesTool
        .execute(input, &context)
        .await
        .expect_err("adapter grep_files input error");
    assert_eq!(actual.to_string(), expected.to_string());
}
