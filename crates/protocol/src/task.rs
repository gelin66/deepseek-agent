//! Canonical task, verifier, evidence, and completion contracts.
//!
//! The model may create a [`CompletionCandidate`], but it cannot create an
//! [`EvidenceReceipt`] or a [`CompletionDecision`]. Those are Host facts
//! validated and persisted by the single Agent runtime.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_HOST_ACCEPTANCE_DESCRIPTION: &str = "由 Host 明确接受完成候选";

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
    };
}

string_id!(TaskGenerationId);
string_id!(AcceptanceId);
string_id!(VerificationId);
string_id!(EvidenceReceiptId);
string_id!(CompletionCandidateId);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TaskDefinition {
    pub objective: String,
    pub constraints: Vec<String>,
    pub non_goals: Vec<String>,
    pub acceptance: Vec<TaskAcceptance>,
}

impl TaskDefinition {
    #[must_use]
    pub fn host(objective: impl Into<String>) -> Self {
        Self {
            objective: objective.into(),
            constraints: Vec::new(),
            non_goals: Vec::new(),
            acceptance: vec![TaskAcceptance::Host {
                id: AcceptanceId::from("host"),
                description: DEFAULT_HOST_ACCEPTANCE_DESCRIPTION.to_owned(),
            }],
        }
    }

    /// Render the frozen user task as one deterministic model-visible turn.
    ///
    /// The common objective-only Host task intentionally preserves the exact
    /// input text. Structured callers get every human semantic field; Host
    /// verifier internals remain typed runtime facts rather than prompt data.
    #[must_use]
    pub fn model_message(&self) -> String {
        let default_host_task = self.constraints.is_empty()
            && self.non_goals.is_empty()
            && matches!(
                self.acceptance.as_slice(),
                [TaskAcceptance::Host { id, description }]
                    if id.0 == "host" && description == DEFAULT_HOST_ACCEPTANCE_DESCRIPTION
            );
        if default_host_task {
            return self.objective.clone();
        }

        let mut message = format!("任务目标：\n{}", self.objective);
        append_model_list(&mut message, "约束", &self.constraints);
        append_model_list(&mut message, "非目标", &self.non_goals);
        message.push_str("\n\n验收条件：");
        for acceptance in &self.acceptance {
            match acceptance {
                TaskAcceptance::Host { description, .. } => {
                    message.push_str("\n- ");
                    message.push_str(description);
                }
                TaskAcceptance::Verifier {
                    description,
                    verifier,
                    ..
                } => {
                    message.push_str("\n- ");
                    message.push_str(description);
                    message.push_str("（Host 将使用 `");
                    message.push_str(&verifier.verifier_id);
                    message.push_str("` 做确定性验证）");
                }
            }
        }
        message
    }

    pub fn validate(&self) -> Result<(), String> {
        require_text("task objective", &self.objective)?;
        validate_text_list("constraint", &self.constraints)?;
        validate_text_list("non-goal", &self.non_goals)?;
        if self.acceptance.is_empty() {
            return Err("task acceptance must not be empty".to_owned());
        }
        let mut ids = HashSet::new();
        let mut verifier_count = 0_usize;
        for acceptance in &self.acceptance {
            acceptance.validate()?;
            verifier_count += usize::from(matches!(acceptance, TaskAcceptance::Verifier { .. }));
            if !ids.insert(acceptance.id().0.as_str()) {
                return Err(format!(
                    "task contains duplicate acceptance id '{}'",
                    acceptance.id().0
                ));
            }
        }
        if verifier_count > 1 {
            return Err(
                "task must use one exact verifier plan; put multiple gates in that plan".to_owned(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TaskContract {
    pub generation_id: TaskGenerationId,
    pub definition: TaskDefinition,
}

impl TaskContract {
    pub fn validate(&self) -> Result<(), String> {
        require_id("task generation id", &self.generation_id.0)?;
        self.definition.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskAcceptance {
    Host {
        id: AcceptanceId,
        description: String,
    },
    Verifier {
        id: AcceptanceId,
        description: String,
        verifier: VerifierSpec,
    },
}

impl TaskAcceptance {
    #[must_use]
    pub fn id(&self) -> &AcceptanceId {
        match self {
            Self::Host { id, .. } | Self::Verifier { id, .. } => id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let (id, description) = match self {
            Self::Host { id, description } => (id, description),
            Self::Verifier {
                id,
                description,
                verifier,
            } => {
                verifier.validate()?;
                (id, description)
            }
        };
        require_id("acceptance id", &id.0)?;
        require_text("acceptance description", description)
    }
}

/// Exact verifier request and Host-resolved execution plan.
///
/// A receipt may satisfy this specification only when both the normalized
/// parameters and every execution-plan field match exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VerifierSpec {
    pub verifier_id: String,
    pub parameters: Value,
    pub plan: VerifierPlan,
}

impl VerifierSpec {
    pub fn validate(&self) -> Result<(), String> {
        require_id("verifier id", &self.verifier_id)?;
        if !self.parameters.is_object() {
            return Err("verifier parameters must be a JSON object".to_owned());
        }
        self.plan.validate()
    }

    #[must_use]
    pub fn canonicalized(mut self) -> Self {
        self.parameters = canonical_json(&self.parameters);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VerifierPlan {
    pub steps: Vec<VerifierStep>,
}

impl VerifierPlan {
    pub fn validate(&self) -> Result<(), String> {
        if self.steps.is_empty() {
            return Err("verifier plan must contain at least one step".to_owned());
        }
        let mut ids = HashSet::new();
        for step in &self.steps {
            step.validate()?;
            if !ids.insert(step.id.as_str()) {
                return Err(format!(
                    "verifier plan contains duplicate step id '{}'",
                    step.id
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VerifierStep {
    pub id: String,
    pub program: String,
    pub args: Vec<String>,
    /// Workspace-relative directory. An empty value means the workspace root.
    pub cwd: String,
    pub env: BTreeMap<String, String>,
    pub timeout_ms: u64,
}

impl VerifierStep {
    pub fn validate(&self) -> Result<(), String> {
        require_id("verifier step id", &self.id)?;
        require_id("verifier program", &self.program)?;
        if self.timeout_ms == 0 {
            return Err("verifier step timeout must be greater than zero".to_owned());
        }
        if self.cwd.starts_with('/')
            || self.cwd.starts_with('\\')
            || self.cwd.split(['/', '\\']).any(|part| part == "..")
        {
            return Err("verifier step cwd must stay inside the workspace".to_owned());
        }
        if self.env.keys().any(|key| key.trim().is_empty()) {
            return Err("verifier environment variable names must not be empty".to_owned());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceRevision {
    Known { sha256: String },
    Unknown { reason: String },
}

impl WorkspaceRevision {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Known { sha256 } => require_id("workspace revision", sha256),
            Self::Unknown { reason } => require_text("workspace revision failure", reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspaceState {
    /// Monotonic Host epoch. It advances whenever a workspace-mutating
    /// operation may have started, even if the content hash later returns to
    /// an earlier value.
    pub generation: u64,
    pub revision: WorkspaceRevision,
}

impl WorkspaceState {
    pub fn validate(&self) -> Result<(), String> {
        self.revision.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierVerdict {
    Passed,
    Failed,
    Partial,
}

/// Typed observation produced by a deterministic verifier implementation.
///
/// It is not trusted completion evidence until AgentRuntime matches it to the
/// frozen contract and seals an [`EvidenceReceipt`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VerifierObservation {
    pub spec: VerifierSpec,
    pub verdict: VerifierVerdict,
    pub workspace_revision: WorkspaceRevision,
    pub artifact_ids: Vec<String>,
}

impl VerifierObservation {
    pub fn validate(&self) -> Result<(), String> {
        self.spec.validate()?;
        self.workspace_revision.validate()?;
        validate_ids("verifier artifact id", &self.artifact_ids)
    }
}

/// Successful, Host-sealed evidence for one frozen acceptance criterion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EvidenceReceipt {
    pub id: EvidenceReceiptId,
    pub generation_id: TaskGenerationId,
    pub acceptance_id: AcceptanceId,
    pub verification_id: VerificationId,
    pub verifier: VerifierSpec,
    pub workspace_state: WorkspaceState,
    pub artifact_ids: Vec<String>,
}

impl EvidenceReceipt {
    pub fn validate(&self) -> Result<(), String> {
        require_id("evidence receipt id", &self.id.0)?;
        require_id("task generation id", &self.generation_id.0)?;
        require_id("acceptance id", &self.acceptance_id.0)?;
        require_id("verification id", &self.verification_id.0)?;
        self.verifier.validate()?;
        self.workspace_state.validate()?;
        if matches!(
            self.workspace_state.revision,
            WorkspaceRevision::Unknown { .. }
        ) {
            return Err("evidence receipt requires a known workspace revision".to_owned());
        }
        if self.artifact_ids.is_empty() {
            return Err("evidence receipt requires at least one artifact".to_owned());
        }
        validate_ids("evidence artifact id", &self.artifact_ids)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompletionCandidate {
    pub id: CompletionCandidateId,
    pub generation_id: TaskGenerationId,
    pub message: String,
}

impl CompletionCandidate {
    pub fn validate(&self) -> Result<(), String> {
        require_id("completion candidate id", &self.id.0)?;
        require_id("task generation id", &self.generation_id.0)?;
        require_text("completion candidate message", &self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AcceptanceSatisfaction {
    Host {
        acceptance_id: AcceptanceId,
    },
    Evidence {
        acceptance_id: AcceptanceId,
        receipt_id: EvidenceReceiptId,
    },
}

impl AcceptanceSatisfaction {
    #[must_use]
    pub fn acceptance_id(&self) -> &AcceptanceId {
        match self {
            Self::Host { acceptance_id } | Self::Evidence { acceptance_id, .. } => acceptance_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompletionDecision {
    pub candidate_id: CompletionCandidateId,
    pub generation_id: TaskGenerationId,
    pub workspace_state: WorkspaceState,
    pub satisfied: Vec<AcceptanceSatisfaction>,
}

impl CompletionDecision {
    pub fn validate(&self) -> Result<(), String> {
        require_id("completion candidate id", &self.candidate_id.0)?;
        require_id("task generation id", &self.generation_id.0)?;
        self.workspace_state.validate()?;
        if self.satisfied.is_empty() {
            return Err("completion decision must satisfy at least one criterion".to_owned());
        }
        let mut ids = HashSet::new();
        for satisfaction in &self.satisfied {
            require_id("acceptance id", &satisfaction.acceptance_id().0)?;
            if let AcceptanceSatisfaction::Evidence { receipt_id, .. } = satisfaction {
                require_id("evidence receipt id", &receipt_id.0)?;
            }
            if !ids.insert(satisfaction.acceptance_id().0.as_str()) {
                return Err(format!(
                    "completion decision satisfies acceptance '{}' more than once",
                    satisfaction.acceptance_id().0
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompletionRejection {
    pub candidate_id: CompletionCandidateId,
    pub unmet_acceptance_ids: Vec<AcceptanceId>,
    pub reason: String,
}

impl CompletionRejection {
    pub fn validate(&self) -> Result<(), String> {
        require_id("completion candidate id", &self.candidate_id.0)?;
        if self.unmet_acceptance_ids.is_empty() {
            return Err("completion rejection must identify unmet acceptance".to_owned());
        }
        validate_ids(
            "unmet acceptance id",
            &self
                .unmet_acceptance_ids
                .iter()
                .map(|id| id.0.clone())
                .collect::<Vec<_>>(),
        )?;
        require_text("completion rejection reason", &self.reason)
    }
}

/// Recursively sort JSON object keys so callers compare the same exact
/// verifier parameters independent of input key order.
#[must_use]
pub fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted = map
                .iter()
                .map(|(key, value)| (key.clone(), canonical_json(value)))
                .collect();
            Value::Object(sorted)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        _ => value.clone(),
    }
}

fn require_id(kind: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{kind} must not be empty"))
    } else {
        Ok(())
    }
}

fn require_text(kind: &str, value: &str) -> Result<(), String> {
    require_id(kind, value)
}

fn validate_text_list(kind: &str, values: &[String]) -> Result<(), String> {
    if values.iter().any(|value| value.trim().is_empty()) {
        return Err(format!("{kind} entries must not be empty"));
    }
    Ok(())
}

fn append_model_list(message: &mut String, title: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    message.push_str("\n\n");
    message.push_str(title);
    message.push('：');
    for value in values {
        message.push_str("\n- ");
        message.push_str(value);
    }
}

fn validate_ids(kind: &str, values: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for value in values {
        require_id(kind, value)?;
        if !seen.insert(value.as_str()) {
            return Err(format!("{kind} '{value}' appears more than once"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn exact_test_spec() -> VerifierSpec {
        VerifierSpec {
            verifier_id: "run_tests".to_owned(),
            parameters: json!({"args": ["--locked"], "all_features": false}),
            plan: VerifierPlan {
                steps: vec![VerifierStep {
                    id: "cargo-test".to_owned(),
                    program: "cargo".to_owned(),
                    args: vec!["test".to_owned(), "--locked".to_owned()],
                    cwd: String::new(),
                    env: BTreeMap::new(),
                    timeout_ms: 600_000,
                }],
            },
        }
    }

    #[test]
    fn task_contract_rejects_empty_and_duplicate_acceptance() {
        let mut definition = TaskDefinition::host("修复边界错误");
        assert!(definition.validate().is_ok());
        definition.acceptance.push(definition.acceptance[0].clone());
        assert_eq!(
            definition.validate().unwrap_err(),
            "task contains duplicate acceptance id 'host'"
        );
        definition.objective.clear();
        assert_eq!(
            definition.validate().unwrap_err(),
            "task objective must not be empty"
        );
    }

    #[test]
    fn task_model_message_preserves_plain_input_and_projects_structured_semantics() {
        assert_eq!(
            TaskDefinition::host("修复边界错误").model_message(),
            "修复边界错误"
        );

        let definition = TaskDefinition {
            objective: "修复边界错误".to_owned(),
            constraints: vec!["只修改 ranges.py".to_owned()],
            non_goals: vec!["不要重写测试".to_owned()],
            acceptance: vec![TaskAcceptance::Verifier {
                id: AcceptanceId::from("tests"),
                description: "全部单元测试通过".to_owned(),
                verifier: exact_test_spec(),
            }],
        };
        assert_eq!(
            definition.model_message(),
            concat!(
                "任务目标：\n修复边界错误",
                "\n\n约束：\n- 只修改 ranges.py",
                "\n\n非目标：\n- 不要重写测试",
                "\n\n验收条件：\n- 全部单元测试通过",
                "（Host 将使用 `run_tests` 做确定性验证）"
            )
        );
    }

    #[test]
    fn verifier_contract_requires_exact_workspace_bound_plan() {
        let mut spec = exact_test_spec();
        assert!(spec.validate().is_ok());
        spec.plan.steps[0].cwd = "../outside".to_owned();
        assert_eq!(
            spec.validate().unwrap_err(),
            "verifier step cwd must stay inside the workspace"
        );
        spec.plan.steps[0].cwd.clear();
        spec.plan.steps[0].timeout_ms = 0;
        assert_eq!(
            spec.validate().unwrap_err(),
            "verifier step timeout must be greater than zero"
        );
    }

    #[test]
    fn canonical_parameters_ignore_object_key_order_but_not_argv_order() {
        let left = canonical_json(&json!({
            "all_features": false,
            "args": ["--package", "a"]
        }));
        let right = canonical_json(&json!({
            "args": ["--package", "a"],
            "all_features": false
        }));
        let different = canonical_json(&json!({
            "args": ["a", "--package"],
            "all_features": false
        }));
        assert_eq!(left, right);
        assert_ne!(left, different);
    }

    #[test]
    fn receipt_requires_known_revision_and_exact_binding() {
        let mut receipt = EvidenceReceipt {
            id: EvidenceReceiptId::from("receipt-1"),
            generation_id: TaskGenerationId::from("run-1"),
            acceptance_id: AcceptanceId::from("tests"),
            verification_id: VerificationId::from("verification-1"),
            verifier: exact_test_spec(),
            workspace_state: WorkspaceState {
                generation: 2,
                revision: WorkspaceRevision::Known {
                    sha256: "revision-2".to_owned(),
                },
            },
            artifact_ids: vec!["artifact-1".to_owned()],
        };
        assert!(receipt.validate().is_ok());
        receipt.workspace_state.revision = WorkspaceRevision::Unknown {
            reason: "Git revision unavailable".to_owned(),
        };
        assert_eq!(
            receipt.validate().unwrap_err(),
            "evidence receipt requires a known workspace revision"
        );
    }

    #[test]
    fn task_wire_rejects_unknown_fields() {
        let value = json!({
            "objective": "修复边界错误",
            "constraints": [],
            "non_goals": [],
            "acceptance": [{
                "kind": "host",
                "id": "host",
                "description": "Host accepts"
            }],
            "legacy_goal": true
        });
        assert!(serde_json::from_value::<TaskDefinition>(value).is_err());
    }
}
