#!/usr/bin/env python3
"""Deterministic offline checks for DSE's public repository surface."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

AGENT_GUIDE = Path("AGENTS.md")
AUTHORITY_INDEX = Path("docs/README.md")
PRODUCT_PLAN = Path("docs/product/PRODUCT_PLAN.md")
ROADMAP = Path("docs/product/ROADMAP.md")
EVALUATION = Path("docs/product/EVALUATION.md")
CURRENT_ARCHITECTURE = Path("docs/architecture/CURRENT_CODEWHALE.md")
DEV_GATE = Path("scripts/dev-dse.sh")

# Frozen at the clean M44 checkpoint (801391577), before ADR-0016 entered this
# worktree: Product Plan + Roadmap + Evaluation + Current Architecture + all
# accepted ADRs required 17,636 lines of unconditional bootstrap reading.
AUTHORITY_BOOTSTRAP_BASELINE_LINES = 17_636
AUTHORITY_BOOTSTRAP_MAX_RATIO = 0.25
AUTHORITY_ROUTE_START = "<!-- authority-routes:start -->"
AUTHORITY_ROUTE_END = "<!-- authority-routes:end -->"
BOOTSTRAP_QUESTION_START = "<!-- bootstrap-questions:start -->"
BOOTSTRAP_QUESTION_END = "<!-- bootstrap-questions:end -->"
EXPECTED_OWNER_ROUTES = {
    "app",
    "runtime",
    "protocol",
    "deepseek",
    "context",
    "tools",
    "state",
    "orchestrator",
    "localization",
    "clients",
    "repository-guidance",
}
EXPECTED_BOOTSTRAP_QUESTIONS = {
    "current_goal",
    "owner",
    "forbidden",
    "focused_gate",
    "full_gate",
    "deletion",
}

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
    "Run API v15",
    "RuntimeEvent v22",
    "State schema v28",
    "exec-stream v6",
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
    "Run API v12",
    "RuntimeEvent v19",
    "State schema v25",
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
    "docs/README.md#owner-routes",
    "docs/decisions/",
    "docs/product/ROADMAP.md#current-execution-window",
)
AGENT_GUIDE_REQUIRED_RULES = (
    "One DeepSeek backend.",
    "One `AgentRuntime` for root and child agents.",
    "one `RunStore`",
    "Changing one of these constraints requires evidence and a new ADR.",
    "Each implementation slice must state:",
    "Do not read every ADR",
    "Never use broad `git clean`",
    "Preserve existing user and agent changes",
    "Do not push, release, force-push",
    "Never rewrite frozen manifests, summaries, raw, or Git history.",
    "Audit against the official DeepSeek protocol",
    "Root and child agents must eventually pass the same conformance suite.",
    "read-only agents may share a view",
    "./scripts/dev-dse.sh authority",
    "./scripts/dev-dse.sh focused",
    "./scripts/dev-dse.sh full",
    "Run the full gate at most",
    "Offline fixtures come before credentials",
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


def marked_table_rows(body: str, start: str, end: str) -> dict[str, tuple[str, ...]]:
    if body.count(start) != 1 or body.count(end) != 1:
        fail(f"authority fixture markers must occur exactly once: {start}, {end}")
    block = body.split(start, maxsplit=1)[1].split(end, maxsplit=1)[0]
    rows: dict[str, tuple[str, ...]] = {}
    for raw_line in block.splitlines():
        line = raw_line.strip()
        if not line.startswith("|"):
            continue
        cells = tuple(cell.strip() for cell in line.strip("|").split("|"))
        if not cells or not cells[0].startswith("`") or not cells[0].endswith("`"):
            continue
        key = cells[0].strip("`")
        if key in rows:
            fail(f"duplicate authority fixture row: {key}")
        rows[key] = cells
    return rows


def local_link_target(source: Path, raw_target: str) -> tuple[Path, str | None]:
    target = raw_target.strip().split(maxsplit=1)[0].strip("<>")
    path_text, separator, fragment = target.partition("#")
    resolved = (ROOT / source if not path_text else ROOT / source.parent / path_text).resolve()
    try:
        relative = resolved.relative_to(ROOT.resolve())
    except ValueError:
        fail(f"{source}: authority link escapes repository: {raw_target}")
    if not (ROOT / relative).is_file():
        fail(f"{source}: authority link is not a file: {raw_target}")
    return relative, fragment if separator else None


def explicit_anchor_line(relative: Path, fragment: str) -> int:
    marker = f'<a id="{fragment}"></a>'
    lines = read(relative).splitlines()
    matches = [index for index, line in enumerate(lines) if line.strip() == marker]
    if len(matches) != 1:
        fail(f"{relative}: expected one explicit authority anchor {marker}")
    return matches[0]


def authority_read_lines(relative: Path, fragment: str | None) -> int:
    lines = read(relative).splitlines()
    if fragment is None:
        return len(lines)

    anchor = explicit_anchor_line(relative, fragment)
    heading_index = None
    heading_level = None
    for index in range(anchor + 1, len(lines)):
        match = re.match(r"^(#{1,6})\s+", lines[index])
        if match:
            heading_index = index
            heading_level = len(match.group(1))
            break
    if heading_index is None or heading_level is None:
        fail(f"{relative}#{fragment}: anchor is not followed by a heading")

    end = len(lines)
    for index in range(heading_index + 1, len(lines)):
        match = re.match(r"^(#{1,6})\s+", lines[index])
        if match and len(match.group(1)) <= heading_level:
            end = index
            break
    return end - anchor


def authority_links_in_cell(source: Path, cell: str) -> tuple[tuple[Path, str | None], ...]:
    links: list[tuple[Path, str | None]] = []
    for raw_target in LINK_RE.findall(cell):
        if "://" in raw_target or raw_target.startswith("mailto:"):
            fail(f"{source}: owner route must use a local authority link: {raw_target}")
        links.append(local_link_target(source, raw_target))
    return tuple(links)


def authority_read_set(
    route_cells: tuple[str, ...],
) -> tuple[tuple[Path, str | None], ...]:
    if len(route_cells) != 5:
        fail("authority route rows must have owner, code owner, ADR, current, evaluation columns")
    mandatory: list[tuple[Path, str | None]] = [
        (AGENT_GUIDE, None),
        (AUTHORITY_INDEX, None),
        (PRODUCT_PLAN, None),
        (ROADMAP, "current-execution-window"),
    ]
    # Evaluation is deliberately conditional. Ordinary owner-scoped bootstrap
    # reads the accepted decisions and current facts columns only.
    mandatory.extend(authority_links_in_cell(AUTHORITY_INDEX, route_cells[2]))
    mandatory.extend(authority_links_in_cell(AUTHORITY_INDEX, route_cells[3]))
    unique: dict[tuple[Path, str | None], None] = {}
    for target in mandatory:
        unique[target] = None
    return tuple(unique)


def check_authority_contract() -> tuple[int, int, str, dict[str, tuple[tuple[Path, str | None], ...]]]:
    agent_body = read(AGENT_GUIDE)
    index_body = read(AUTHORITY_INDEX)
    gate_body = read(DEV_GATE)

    if "Read completely, in order, before changing the repository:" in agent_body:
        fail("unconditional full-read bootstrap remains in AGENTS.md")
    if "cargo clippy --workspace --all-targets --locked -- -D warnings" in agent_body:
        fail("AGENTS.md duplicates the executable full gate owned by scripts/dev-dse.sh")

    route_rows = marked_table_rows(index_body, AUTHORITY_ROUTE_START, AUTHORITY_ROUTE_END)
    if set(route_rows) != EXPECTED_OWNER_ROUTES:
        missing = sorted(EXPECTED_OWNER_ROUTES - set(route_rows))
        extra = sorted(set(route_rows) - EXPECTED_OWNER_ROUTES)
        fail(f"owner routes mismatch; missing={missing}, extra={extra}")
    for owner, cells in route_rows.items():
        if len(cells) != 5:
            fail(f"authority route has wrong column count: {owner}")
        for cell in cells[2:5]:
            links = authority_links_in_cell(AUTHORITY_INDEX, cell)
            if not links:
                fail(f"authority route has an empty authority column: {owner}")
            for relative, fragment in links:
                if fragment is not None:
                    explicit_anchor_line(relative, fragment)

    question_rows = marked_table_rows(
        index_body, BOOTSTRAP_QUESTION_START, BOOTSTRAP_QUESTION_END
    )
    if set(question_rows) != EXPECTED_BOOTSTRAP_QUESTIONS:
        missing = sorted(EXPECTED_BOOTSTRAP_QUESTIONS - set(question_rows))
        extra = sorted(set(question_rows) - EXPECTED_BOOTSTRAP_QUESTIONS)
        fail(f"bootstrap questions mismatch; missing={missing}, extra={extra}")
    for key, cells in question_rows.items():
        if len(cells) != 2 or not authority_links_in_cell(AUTHORITY_INDEX, cells[1]):
            fail(f"bootstrap question has no exact authority answer: {key}")

    decision_files = tuple(
        path.relative_to(ROOT)
        for path in sorted(
            (ROOT / "docs/decisions").glob("[0-9][0-9][0-9][0-9]-*.md")
        )
    )
    for relative in decision_files:
        decision_body = read(relative)
        if "- 状态：已接受" not in decision_body and "- 状态：已被" not in decision_body:
            fail(f"decision index contains an ADR without accepted/superseded status: {relative}")
        expected_link = str(relative.relative_to(Path("docs")))
        if f"]({expected_link})" not in index_body:
            fail(f"accepted decision is not reachable from docs/README.md: {relative}")

    required_anchors = (
        (AGENT_GUIDE, "product-boundary"),
        (AGENT_GUIDE, "owner-map"),
        (AGENT_GUIDE, "development-method"),
        (AGENT_GUIDE, "risk-tier-gate"),
        (AUTHORITY_INDEX, "owner-routes"),
        (ROADMAP, "current-execution-window"),
    )
    for relative, fragment in required_anchors:
        explicit_anchor_line(relative, fragment)

    reachability = {
        "product_plan": "](docs/product/PRODUCT_PLAN.md)" in agent_body,
        "owner_map": '<a id="owner-map"></a>' in agent_body,
        "current_milestone": "](docs/product/ROADMAP.md#current-execution-window)" in agent_body,
        "focused_gate": "./scripts/dev-dse.sh focused" in agent_body,
        "full_gate": "./scripts/dev-dse.sh full" in agent_body,
        "deletion_rule": "A replacement slice deletes its old path after cutover." in agent_body,
    }
    for relative in decision_files:
        reachability[f"accepted:{relative.name}"] = (
            f"]({relative.relative_to(Path('docs'))})" in index_body
        )
    missing_reachability = sorted(key for key, reachable in reachability.items() if not reachable)
    if missing_reachability:
        fail("fixed boundary is unreachable: " + ", ".join(missing_reachability))

    for marker in ("Risk 0", "Risk 1", "Risk 2", "Risk 3", "Risk 4"):
        if marker not in agent_body:
            fail(f"risk-tier gate is missing {marker}")
    for mode in ("authority)", "focused)", "full)"):
        if mode not in gate_body:
            fail(f"scripts/dev-dse.sh is missing canonical gate mode {mode[:-1]}")
    for command in (
        "cargo clippy --workspace --all-targets --locked -- -D warnings",
        "cargo test --workspace --locked",
        "git diff --check",
    ):
        if gate_body.count(command) != 1:
            fail(f"full gate command must have one executable owner: {command}")

    route_sets: dict[str, tuple[tuple[Path, str | None], ...]] = {}
    route_lines: dict[str, int] = {}
    for owner, cells in route_rows.items():
        read_set = authority_read_set(cells)
        route_sets[owner] = read_set
        route_lines[owner] = sum(
            authority_read_lines(relative, fragment) for relative, fragment in read_set
        )
    max_owner = max(route_lines, key=route_lines.__getitem__)
    max_lines = route_lines[max_owner]
    ceiling = int(AUTHORITY_BOOTSTRAP_BASELINE_LINES * AUTHORITY_BOOTSTRAP_MAX_RATIO)
    if max_lines > ceiling:
        fail(
            f"owner-scoped bootstrap exceeds 25% ceiling: "
            f"owner={max_owner}, lines={max_lines}, ceiling={ceiling}"
        )

    return len(reachability), max_lines, max_owner, route_sets


def print_authority_report(
    reachable: int,
    max_lines: int,
    max_owner: str,
    route_sets: dict[str, tuple[tuple[Path, str | None], ...]],
) -> None:
    ceiling = int(AUTHORITY_BOOTSTRAP_BASELINE_LINES * AUTHORITY_BOOTSTRAP_MAX_RATIO)
    for owner in sorted(route_sets):
        read_set = route_sets[owner]
        lines = sum(authority_read_lines(path, fragment) for path, fragment in read_set)
        rendered = ", ".join(
            f"{path}{'#' + fragment if fragment else ''}" for path, fragment in read_set
        )
        print(f"authority route {owner}: {lines} lines :: {rendered}")
    print(
        "authority bootstrap report: "
        f"baseline={AUTHORITY_BOOTSTRAP_BASELINE_LINES}, ceiling={ceiling}, "
        f"max_owner={max_owner}, max_lines={max_lines}"
    )
    print(f"fixed boundary reachability: {reachable}/{reachable} (100%)")


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
        if not (ROOT / relative).exists():
            # A replacement slice may delete a tracked source before the
            # reviewable commit is staged. Missing paths have no active
            # identity surface and disappear from git ls-files after cutover.
            continue
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
        if not (ROOT / relative).exists():
            continue
        if relative.suffix != ".rs":
            continue
        for line_number, line in enumerate(read(relative).splitlines(), start=1):
            if not RETIRED_VISUAL_RE.search(line):
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
    authority_report = check_authority_contract()
    if sys.argv[1:] == ["--authority-only"]:
        print_authority_report(*authority_report)
        print("authority check passed")
        return 0
    if sys.argv[1:]:
        fail("usage: check-public-repository.py [--authority-only]")

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
    print_authority_report(*authority_report)
    print("public repository check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
