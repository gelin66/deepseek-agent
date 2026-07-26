#!/usr/bin/env python3
"""Deterministic offline checks for DSE's public repository surface."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

AGENT_GUIDE = Path("AGENTS.md")

MARKDOWN_FILES = (
    Path("README.md"),
    Path("README.zh-CN.md"),
    Path("CONTRIBUTING.md"),
    Path("SECURITY.md"),
    Path("CODE_OF_CONDUCT.md"),
    Path("THIRD_PARTY_NOTICES.md"),
    Path(".github/PULL_REQUEST_TEMPLATE.md"),
)

CURRENT_REFERENCE_FILES = (
    Path("docs/reference/ACCESSIBILITY.md"),
    Path("docs/reference/CONFIGURATION.md"),
    Path("docs/reference/MCP.md"),
    Path("docs/reference/OPERATIONS_RUNBOOK.md"),
    Path("docs/reference/SANDBOX.md"),
)

HISTORICAL_IDENTITY_FACTS = {
    Path("docs/product/ROADMAP.md"): (
        "[M7-A DeepSeek Agent 收敛正式 A/B v1]",
        "[M7-A2 DeepSeek Agent 收敛正式 A/B]",
    ),
    Path("docs/product/EVALUATION.md"): (
        "[M7-A DeepSeek Agent 收敛正式 A/B v1]",
        "[M7-A2 DeepSeek Agent 收敛正式 A/B]",
        "历史 `M7-A/M7-A2 DeepSeek Agent` 标题",
    ),
    Path("eval/summaries/m7-a-agent-convergence-ab-2026-07-22.md"): (
        "# M7-A DeepSeek Agent 收敛正式 A/B v1",
    ),
    Path("eval/summaries/m7-a2-agent-convergence-ab-2026-07-22.md"): (
        "# M7-A2 DeepSeek Agent 收敛正式 A/B",
    ),
}

REQUIRED_FILES = MARKDOWN_FILES + (
    Path("LICENSE"),
    Path(".env.example"),
    Path(".github/CODEOWNERS"),
    Path(".github/ISSUE_TEMPLATE/bug_report.yml"),
    Path(".github/ISSUE_TEMPLATE/feature_request.yml"),
) + CURRENT_REFERENCE_FILES

README_IDENTIFIERS = (
    "DSE",
    "DeepSeek Engineer",
    "https://api.deepseek.com/chat/completions",
    "deepseek-v4-pro",
    "deepseek-v4-flash",
    "Run API v12",
    "RuntimeEvent v19",
    "State schema v25",
    "exec-stream v4",
    "dse exec --auto",
    "~/.dse/config.toml",
    "DSE_HOME",
    "DSE_CONFIG_PATH",
    "DEEPSEEK_API_KEY",
    "DEEPSEEK_BASE_URL",
    "DEEPSEEK_MODEL",
    "TaskContract",
    "EvidenceReceipt",
    "AgentRuntime",
    "RunStore",
    "THIRD_PARTY_NOTICES.md",
)

FORBIDDEN_CURRENT_FACTS = (
    "Run API v10",
    "RuntimeEvent v16",
    "State schema v21",
    "exec-stream v2",
    "DEEPSEEK_PROVIDER",
    "NVIDIA_NIM",
    "ATLASCLOUD",
    "~/.deepseek",
    "DeepSeek TUI environment",
    "current milestone is M8",
    "当前里程碑是 M8",
)

LINK_RE = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
BASH_BLOCK_RE = re.compile(r"```bash\n(.*?)\n```", re.DOTALL)
SECRET_RE = re.compile(
    r"(?:sk-[A-Za-z0-9_-]{16,}|"
    r"gh[pousr]_[A-Za-z0-9_]{20,}|"
    r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----)"
)

LEGACY_SOURCE_RE = re.compile(r"CodeWhale|codewhale|CODEWHALE_|\.codewhale")
RETIRED_VISUAL_RE = re.compile(r"\bwhale\b|whale_", re.IGNORECASE)
AGENT_GUIDE_HISTORY_RE = re.compile(
    r"(?m)^## Current repository truth\s*$|"
    r"\bM\d+(?:-[A-Z0-9]+)*\b|"
    r"`[0-9a-f]{8,40}`"
)
AGENT_GUIDE_MAX_LINES = 140
AGENT_GUIDE_MAX_BYTES = 9_000
AGENT_GUIDE_REQUIRED_LINKS = (
    "docs/product/PRODUCT_PLAN.md",
    "docs/decisions/",
    "docs/product/ROADMAP.md",
    "docs/product/EVALUATION.md",
    "docs/architecture/CURRENT_CODEWHALE.md",
)
AGENT_GUIDE_REQUIRED_RULES = (
    "One DeepSeek backend.",
    "One `AgentRuntime` for root and child agents.",
    "one `RunStore`",
    "Changing one of these constraints requires evidence and a new ADR.",
    "Each implementation slice must state:",
    "Never use broad `git clean`",
    "Preserve existing user and agent changes",
    "Do not push, release, force-push",
    "Never rewrite frozen manifests, summaries, raw, or Git history.",
    "Audit against the official DeepSeek protocol",
    "Root and child agents must eventually pass the same conformance suite.",
    "read-only agents may share a view",
    "./scripts/dev-dse.sh focused",
    "cargo clippy --workspace --all-targets --locked -- -D warnings",
    "git diff --check",
    "Update `ROADMAP.md` for milestone status",
)
CRATE_LEGACY_ALLOWLIST = {
    Path("crates/app-server/src/lib.rs"): (
        re.compile(r'"codewhale-(?:core|state|tools|agent|config)"'),
        re.compile(r'"deploy/tencent-lighthouse/systemd/codewhale-runtime\.service"'),
    ),
    Path("crates/app-server/tests/process_crash_recovery.rs"): (
        re.compile(r'"schema": "codewhale\.eval\.m4b-app-server-recovery\.v1"'),
    ),
    Path("crates/cli/src/lib.rs"): (
        re.compile(r'assert!\(!help\.contains\("(?:CodeWhale|codewhale (?:exec|app-server))"\)'),
    ),
    Path("crates/config/src/tests.rs"): (
        re.compile(r"retired-codewhale-(?:home|config\.toml)"),
        re.compile(r'ScopedEnv::set\("CODEWHALE_(?:HOME|CONFIG_PATH)"'),
    ),
    Path("crates/context/src/prompts.rs"): (
        re.compile(r'assert!\(!prompt\.blocks\[0\]\.text\.contains\("CodeWhale"\)\)'),
    ),
    Path("crates/protocol/tests/fixtures/canonical-json-v1.json"): (
        re.compile(r'"schema": "codewhale\.protocol\.canonical-json\.v1"'),
    ),
    Path("crates/state/src/lib.rs"): (
        re.compile(r"// CodeWhale to DSE\."),
    ),
    Path("crates/state/tests/run_store.rs"): (
        re.compile(r'request\("v18-materialized-codewhale-run"'),
    ),
}


def fail(message: str) -> None:
    print(f"public repository check failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def read(relative: Path) -> str:
    path = ROOT / relative
    if not path.is_file():
        fail(f"missing required file: {relative}")
    return path.read_text(encoding="utf-8")


def tracked_files(*pathspecs: str) -> tuple[Path, ...]:
    proc = subprocess.run(
        ["git", "ls-files", "--", *pathspecs],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        fail(f"git ls-files failed: {proc.stderr.strip() or proc.returncode}")
    return tuple(Path(line) for line in proc.stdout.splitlines() if line)


def check_required_files() -> None:
    for relative in REQUIRED_FILES:
        if not (ROOT / relative).is_file():
            fail(f"missing required file: {relative}")


def agent_guide_contract_errors(body: str) -> tuple[str, ...]:
    errors: list[str] = []
    if len(body.splitlines()) > AGENT_GUIDE_MAX_LINES:
        errors.append("line_budget_exceeded")
    if len(body.encode("utf-8")) > AGENT_GUIDE_MAX_BYTES:
        errors.append("byte_budget_exceeded")

    link_targets = {
        raw.strip().split(maxsplit=1)[0].strip("<>")
        for raw in LINK_RE.findall(body)
    }
    for target in AGENT_GUIDE_REQUIRED_LINKS:
        if target not in link_targets:
            errors.append(f"missing_authority_link:{target}")
    for rule in AGENT_GUIDE_REQUIRED_RULES:
        if rule not in body:
            errors.append(f"missing_stable_rule:{rule}")
    if AGENT_GUIDE_HISTORY_RE.search(body):
        errors.append("mutable_history")
    return tuple(errors)


def check_agent_guide_contract() -> None:
    body = read(AGENT_GUIDE)
    errors = agent_guide_contract_errors(body)
    if errors:
        fail(f"{AGENT_GUIDE}: " + ", ".join(errors))


def check_agent_guide_validator() -> None:
    body = read(AGENT_GUIDE)
    fixtures = (
        (
            "oversized guide",
            body + ("\nfixture line" * (AGENT_GUIDE_MAX_LINES + 1)),
            "line_budget_exceeded",
        ),
        (
            "missing authority link",
            body.replace(
                "](docs/product/PRODUCT_PLAN.md)",
                "](docs/product/PRODUCT_PLAN.invalid)",
                1,
            ),
            "missing_authority_link:docs/product/PRODUCT_PLAN.md",
        ),
        (
            "mutable milestone history",
            body + "\n## Current repository truth\n- M25 candidate `deadbeef`\n",
            "mutable_history",
        ),
    )
    if agent_guide_contract_errors(body):
        fail("agent-guide validator rejected the canonical guide")
    for name, fixture, expected in fixtures:
        if expected not in agent_guide_contract_errors(fixture):
            fail(f"agent-guide validator false green: {name}")


def check_readme_contract() -> None:
    english = read(Path("README.md"))
    chinese = read(Path("README.zh-CN.md"))

    for identifier in README_IDENTIFIERS:
        if identifier not in english:
            fail(f"README.md is missing current fact: {identifier}")
        if identifier not in chinese:
            fail(f"README.zh-CN.md is missing current fact: {identifier}")
    if "README.zh-CN.md" not in english or "README.md" not in chinese:
        fail("README language entries do not link to each other")

    public_current = "\n".join(
        read(path)
        for path in (
            Path("README.md"),
            Path("README.zh-CN.md"),
            Path("CONTRIBUTING.md"),
            Path(".env.example"),
        )
    )
    for stale in FORBIDDEN_CURRENT_FACTS:
        if stale in public_current:
            fail(f"stale or retired current-product fact remains: {stale}")

    if re.search(r"\bDSA\b", public_current):
        fail("superseded DSA product identity remains on the current public surface")

    current_references = "\n".join(read(path) for path in CURRENT_REFERENCE_FILES)
    for stale in ("CodeWhale", "codewhale", "CODEWHALE_", "~/.codewhale", ".codewhale/"):
        if stale in current_references:
            fail(f"retired identity remains in current reference docs: {stale}")


def check_historical_identity_allowlist() -> None:
    for relative, required_facts in HISTORICAL_IDENTITY_FACTS.items():
        body = read(relative)
        for fact in required_facts:
            if fact not in body:
                fail(f"{relative}: historical identity fact changed: {fact}")
        if relative.parts[:2] == ("eval", "summaries") and "DeepSeek Engineer" in body:
            fail(f"{relative}: current DSE identity was written into frozen history")


def check_active_identity_allowlist() -> None:
    retired_tracked = tuple(
        path for path in tracked_files(".codewhale") if (ROOT / path).exists()
    )
    if retired_tracked:
        fail(
            "retired .codewhale product path remains tracked: "
            + ", ".join(map(str, retired_tracked))
        )

    source_files = tracked_files(
        "Cargo.toml",
        "Cargo.lock",
        "config.example.toml",
        ".github",
        "crates",
    )
    for relative in source_files:
        if relative.suffix not in {".json", ".md", ".rs", ".toml", ".yml", ".yaml"}:
            continue
        body = read(relative)
        for line_number, line in enumerate(body.splitlines(), start=1):
            if not LEGACY_SOURCE_RE.search(line):
                continue
            allowed = CRATE_LEGACY_ALLOWLIST.get(relative, ())
            if not any(pattern.search(line) for pattern in allowed):
                fail(
                    f"{relative}:{line_number}: retired identity is not an "
                    "explicit migration/rejection/frozen-fixture fact"
                )
        if re.search(r"\bDSA\b", body):
            fail(f"{relative}: superseded DSA identity remains in active source")

    for relative in tracked_files("crates/tui"):
        if relative.suffix != ".rs":
            continue
        for line_number, line in enumerate(read(relative).splitlines(), start=1):
            if not RETIRED_VISUAL_RE.search(line):
                continue
            if (
                relative == Path("crates/tui/src/palette/tests.rs")
                and 'normalize_theme_name("whale"), None' in line
            ):
                continue
            fail(f"{relative}:{line_number}: retired whale visual identity remains active")


def check_local_links() -> None:
    for relative in (
        MARKDOWN_FILES
        + CURRENT_REFERENCE_FILES
        + (AGENT_GUIDE, Path("docs/README.md"))
    ):
        body = read(relative)
        for raw_target in LINK_RE.findall(body):
            target = raw_target.strip().split(maxsplit=1)[0].strip("<>")
            if (
                not target
                or target.startswith("#")
                or "://" in target
                or target.startswith("mailto:")
            ):
                continue
            path_text = target.split("#", maxsplit=1)[0]
            resolved = (ROOT / relative.parent / path_text).resolve()
            try:
                resolved.relative_to(ROOT.resolve())
            except ValueError:
                fail(f"{relative}: local link escapes repository: {target}")
            if not resolved.exists():
                fail(f"{relative}: broken local link: {target}")


def check_bash_blocks() -> None:
    for relative in MARKDOWN_FILES:
        for index, block in enumerate(BASH_BLOCK_RE.findall(read(relative)), start=1):
            proc = subprocess.run(
                ["/bin/bash", "-n"],
                input=block,
                text=True,
                capture_output=True,
                check=False,
            )
            if proc.returncode != 0:
                detail = proc.stderr.strip() or "unknown shell syntax error"
                fail(f"{relative}: bash block {index} is invalid: {detail}")


def check_templates() -> None:
    codeowners = read(Path(".github/CODEOWNERS"))
    if "* @gelin66" not in codeowners:
        fail("CODEOWNERS does not require repository-owner review")

    for relative in (
        Path(".github/ISSUE_TEMPLATE/bug_report.yml"),
        Path(".github/ISSUE_TEMPLATE/feature_request.yml"),
    ):
        body = read(relative)
        for key in ("name:", "description:", "body:"):
            if key not in body:
                fail(f"{relative}: missing issue-template key {key}")
        if "README" not in body and relative.name == "feature_request.yml":
            fail(f"{relative}: missing public documentation direction")


def check_public_secret_placeholders() -> None:
    public_files = REQUIRED_FILES + (Path("config.example.toml"),)
    for relative in public_files:
        body = read(relative)
        match = SECRET_RE.search(body)
        if match:
            fail(f"{relative}: credential-like material found: {match.group(0)[:8]}…")

    env_example = read(Path(".env.example"))
    if re.search(r"(?m)^[^#\n]*API_KEY\s*=\s*\S+", env_example):
        fail(".env.example contains an active API key assignment")


def main() -> int:
    check_required_files()
    check_agent_guide_contract()
    check_agent_guide_validator()
    check_readme_contract()
    check_historical_identity_allowlist()
    check_active_identity_allowlist()
    check_local_links()
    check_bash_blocks()
    check_templates()
    check_public_secret_placeholders()
    print("public repository check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
