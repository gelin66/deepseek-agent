use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use codewhale_context::{WorkingSetBudget, WorkingSetRequest, select_working_set};
use codewhale_protocol::task::TaskDefinition;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MANIFEST: &str =
    include_str!("../../../eval/manifests/m10-b-working-set-localization-v1.json");

#[derive(Debug, Deserialize)]
struct Manifest {
    schema: String,
    product_metric_eligible: bool,
    official_api_required: bool,
    fixture: Fixture,
    selector: Selector,
    tasks: Vec<Task>,
    decision_rule: DecisionRule,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    path: String,
    tree_sha256: String,
}

#[derive(Debug, Deserialize)]
struct Selector {
    policy_version: String,
    budget: Budget,
}

#[derive(Debug, Deserialize)]
struct Budget {
    max_regions: usize,
    max_total_lines: usize,
    max_region_lines: usize,
    max_scanned_files: usize,
    max_scanned_bytes: usize,
    max_file_bytes: usize,
    max_rendered_chars: usize,
}

impl From<&Budget> for WorkingSetBudget {
    fn from(value: &Budget) -> Self {
        Self {
            max_regions: value.max_regions,
            max_total_lines: value.max_total_lines,
            max_region_lines: value.max_region_lines,
            max_scanned_files: value.max_scanned_files,
            max_scanned_bytes: value.max_scanned_bytes,
            max_file_bytes: value.max_file_bytes,
            max_rendered_chars: value.max_rendered_chars,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Task {
    id: String,
    objective: String,
    changed_paths: Vec<String>,
    relevant_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DecisionRule {
    recall_at_k: usize,
    minimum_recall_at_k: f64,
    minimum_mean_precision_at_k: f64,
    maximum_median_first_relevant_rank: f64,
    maximum_abstention_regions: usize,
    deterministic_repeats: usize,
    production_admission: bool,
}

#[test]
fn frozen_localization_baseline_meets_budget_recall_and_abstention_contract() {
    let manifest: Manifest = serde_json::from_str(MANIFEST).expect("valid frozen manifest");
    assert_eq!(
        manifest.schema,
        "codewhale.eval.m10-b-working-set-localization.v1"
    );
    assert!(!manifest.product_metric_eligible);
    assert!(!manifest.official_api_required);
    assert!(!manifest.decision_rule.production_admission);
    assert!(manifest.decision_rule.deterministic_repeats >= 2);

    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(&manifest.fixture.path);
    assert_eq!(
        fixture_tree_sha256(&repository_root),
        manifest.fixture.tree_sha256
    );
    let workspace = tempfile::tempdir().expect("temporary Git repository");
    copy_tree(&repository_root, workspace.path());
    initialize_git_repository(workspace.path());

    let budget = WorkingSetBudget::from(&manifest.selector.budget);
    let mut recalls = Vec::new();
    let mut precisions = Vec::new();
    let mut first_relevant_ranks = Vec::new();
    let mut abstention_regions = 0_usize;

    for task in &manifest.tasks {
        let definition = TaskDefinition::host(task.objective.clone());
        let request = || WorkingSetRequest {
            workspace: workspace.path(),
            task: &definition,
            changed_paths: &task.changed_paths,
            budget,
        };
        let projection = select_working_set(request()).expect("deterministic selection");
        eprintln!(
            "task={} regions={:?}",
            task.id,
            projection
                .regions
                .iter()
                .map(|region| {
                    (
                        &region.path,
                        region.score,
                        &region.reasons,
                        &region.evidence,
                    )
                })
                .collect::<Vec<_>>()
        );
        for _ in 1..manifest.decision_rule.deterministic_repeats {
            assert_eq!(
                projection,
                select_working_set(request()).expect("repeat selection"),
                "task {} was not deterministic",
                task.id
            );
        }
        assert_eq!(
            projection.policy_version, manifest.selector.policy_version,
            "task {} used an unexpected selector policy",
            task.id
        );
        assert!(projection.regions.len() <= budget.max_regions);
        assert!(
            projection
                .regions
                .iter()
                .map(|region| region.end_line - region.start_line + 1)
                .sum::<usize>()
                <= budget.max_total_lines
        );
        assert!(
            projection
                .render_prompt_block(budget.max_rendered_chars)
                .is_none_or(|block| block.len() <= budget.max_rendered_chars)
        );
        assert!(projection.regions.iter().all(|region| {
            region.sha256.starts_with("sha256:")
                && region.expand_hint.starts_with("read_file path=")
                && region.start_line <= region.end_line
        }));

        if task.relevant_paths.is_empty() {
            abstention_regions += projection.regions.len();
            continue;
        }

        let top = projection
            .regions
            .iter()
            .take(manifest.decision_rule.recall_at_k)
            .map(|region| region.path.as_str())
            .collect::<BTreeSet<_>>();
        let recalled = task
            .relevant_paths
            .iter()
            .filter(|path| top.contains(path.as_str()))
            .count();
        recalls.push(recalled as f64 / task.relevant_paths.len() as f64);
        precisions.push(recalled as f64 / top.len().max(1) as f64);
        first_relevant_ranks.push(
            projection
                .regions
                .iter()
                .position(|region| task.relevant_paths.contains(&region.path))
                .map_or(usize::MAX, |index| index + 1),
        );
    }

    let mean_recall = recalls.iter().sum::<f64>() / recalls.len() as f64;
    let mean_precision = precisions.iter().sum::<f64>() / precisions.len() as f64;
    first_relevant_ranks.sort_unstable();
    let median_first_rank = first_relevant_ranks[first_relevant_ranks.len() / 2] as f64;
    assert!(
        mean_recall >= manifest.decision_rule.minimum_recall_at_k,
        "Recall@{} was {mean_recall:.3}",
        manifest.decision_rule.recall_at_k
    );
    assert!(
        mean_precision >= manifest.decision_rule.minimum_mean_precision_at_k,
        "mean precision@{} was {mean_precision:.3}",
        manifest.decision_rule.recall_at_k
    );
    assert!(
        median_first_rank <= manifest.decision_rule.maximum_median_first_relevant_rank,
        "median first relevant rank was {median_first_rank}"
    );
    assert!(
        abstention_regions <= manifest.decision_rule.maximum_abstention_regions,
        "negative task produced {abstention_regions} regions"
    );
}

fn copy_tree(source: &Path, destination: &Path) {
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("fixture entry");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("fixture type").is_dir() {
            fs::create_dir_all(&target).expect("create fixture directory");
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy fixture file");
        }
    }
}

fn initialize_git_repository(workspace: &Path) {
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "CodeWhale Eval"],
        vec!["config", "user.email", "eval@codewhale.invalid"],
        vec!["add", "."],
        vec!["commit", "-q", "-m", "fixture"],
    ] {
        let status = Command::new("git")
            .args(args)
            .current_dir(workspace)
            .status()
            .expect("git fixture command");
        assert!(status.success());
    }
}

fn fixture_tree_sha256(root: &Path) -> String {
    fn collect(root: &Path, current: &Path, paths: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(current).expect("read fixture tree") {
            let entry = entry.expect("fixture tree entry");
            if entry.file_type().expect("fixture type").is_dir() {
                collect(root, &entry.path(), paths);
            } else {
                paths.push(
                    entry
                        .path()
                        .strip_prefix(root)
                        .expect("fixture-relative path")
                        .to_path_buf(),
                );
            }
        }
    }

    let mut paths = Vec::new();
    collect(root, root, &mut paths);
    paths.sort();
    let mut digest = Sha256::new();
    for relative in paths {
        digest.update(relative.to_string_lossy().as_bytes());
        digest.update([0]);
        digest.update(fs::read(root.join(&relative)).expect("read fixture bytes"));
        digest.update([0]);
    }
    let finalized = digest.finalize();
    let mut rendered = String::from("sha256:");
    for byte in finalized {
        write!(&mut rendered, "{byte:02x}").expect("write digest");
    }
    rendered
}
