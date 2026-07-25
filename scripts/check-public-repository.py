#!/usr/bin/env python3
"""Deterministic offline checks for DSE's public repository surface."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

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


def fail(message: str) -> None:
    print(f"public repository check failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def read(relative: Path) -> str:
    path = ROOT / relative
    if not path.is_file():
        fail(f"missing required file: {relative}")
    return path.read_text(encoding="utf-8")


def check_required_files() -> None:
    for relative in REQUIRED_FILES:
        if not (ROOT / relative).is_file():
            fail(f"missing required file: {relative}")


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


def check_local_links() -> None:
    for relative in MARKDOWN_FILES + CURRENT_REFERENCE_FILES + (Path("docs/README.md"),):
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
    check_readme_contract()
    check_historical_identity_allowlist()
    check_local_links()
    check_bash_blocks()
    check_templates()
    check_public_secret_placeholders()
    print("public repository check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
