//! Exact, snapshot-backed Skill loading for the fixed production catalog.

use dse_context::skills::SkillRegistry;
use serde_json::{Value, json};

use crate::{ToolError, ToolOutcome, required_str};

pub(crate) fn preflight_load_skill(
    input: &Value,
    registry: &SkillRegistry,
) -> Result<(), ToolError> {
    let name = required_str(input, "name")?;
    if registry.get_exact(name).is_none() {
        return Err(ToolError::invalid_input(format!(
            "Skill `{name}` 不在本次运行冻结的可用目录中；请只使用系统提示中列出的精确名称"
        )));
    }
    Ok(())
}

pub(crate) fn execute_load_skill(
    input: Value,
    registry: &SkillRegistry,
) -> Result<ToolOutcome, ToolError> {
    preflight_load_skill(&input, registry)?;
    let name = required_str(&input, "name")?;
    let skill = registry
        .get_exact(name)
        .expect("preflight guarantees exact snapshot membership");
    ToolOutcome::json(&json!({
        "name": skill.name,
        "description": skill.description,
        "body": skill.body,
        "source_path": skill.path,
        "source_sha256": skill.source_sha256,
        "source_bytes": skill.source_bytes,
        "bytes_returned": skill.body.len(),
        "truncated": false,
        "trust": "external_untrusted",
    }))
    .map_err(|error| ToolError::execution_failed(error.to_string()))
}
