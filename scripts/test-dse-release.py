#!/usr/bin/env python3
"""Deterministic release-assembly safety matrix."""

from __future__ import annotations

import copy
import hashlib
import io
import json
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
DELIVERY = REPO_ROOT / "scripts" / "dse-delivery.sh"
RELEASE_TOOL = REPO_ROOT / "scripts" / "dse-release.py"
VERSION = "9.8.7"
REVISION = "9" * 40
TREE = "8" * 40
TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(*arguments: str | Path, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(argument) for argument in arguments],
        cwd=REPO_ROOT,
        check=check,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def fixture_binaries(root: Path) -> Path:
    root.mkdir(parents=True)
    for name in ("dse", "dse-tui"):
        path = root / name
        path.write_text(
            "#!/bin/sh\n"
            f"printf '%s\\n' '{name} {VERSION} release-fixture'\n",
            encoding="utf-8",
        )
        path.chmod(0o755)
    return root


def build_archives(dist: Path, binaries: Path) -> None:
    dist.mkdir(parents=True)
    for target in TARGETS:
        run(
            DELIVERY,
            "package",
            "--output-dir",
            dist,
            "--binary-dir",
            binaries,
            "--version",
            VERSION,
            "--revision",
            REVISION,
            "--source-tree",
            TREE,
            "--target",
            target,
            "--release-asset",
        )


def copy_inputs(source: Path, destination: Path) -> None:
    destination.mkdir()
    for target in TARGETS:
        name = f"dse-{VERSION}-{target}.tar.gz"
        shutil.copy2(source / name, destination / name)
        shutil.copy2(source / f"{name}.sha256", destination / f"{name}.sha256")


def read_archive(path: Path) -> list[tuple[tarfile.TarInfo, bytes | None]]:
    result: list[tuple[tarfile.TarInfo, bytes | None]] = []
    with tarfile.open(path, "r:gz") as archive:
        for member in archive.getmembers():
            extracted = archive.extractfile(member) if member.isfile() else None
            result.append((copy.copy(member), extracted.read() if extracted else None))
    return result


def write_archive(path: Path, members: list[tuple[tarfile.TarInfo, bytes | None]]) -> None:
    with tarfile.open(path, "w:gz") as archive:
        for member, content in members:
            if content is None:
                archive.addfile(member)
            else:
                member.size = len(content)
                archive.addfile(member, io.BytesIO(content))
    path.with_name(f"{path.name}.sha256").write_text(
        f"{sha256(path)}  {path.name}\n", encoding="utf-8"
    )


def mutate_symlink(path: Path) -> None:
    members = read_archive(path)
    for member, _ in members:
        if member.name.endswith("/bin/dse"):
            member.type = tarfile.SYMTYPE
            member.linkname = "dse-tui"
            member.size = 0
            break
    write_archive(
        path,
        [(member, None if member.issym() else content) for member, content in members],
    )


def mutate_traversal(path: Path) -> None:
    members = read_archive(path)
    for member, _ in members:
        if member.name.endswith("/LICENSE"):
            root = member.name.split("/", 1)[0]
            member.name = f"{root}/../escape"
            break
    write_archive(path, members)


def mutate_wrong_target(path: Path) -> None:
    members = read_archive(path)
    manifest_name = next(
        member.name for member, _ in members if member.name.endswith("/manifest.tsv")
    )
    sums_name = next(
        member.name for member, _ in members if member.name.endswith("/SHA256SUMS")
    )
    updated_manifest = b""
    for index, (member, content) in enumerate(members):
        if member.name == manifest_name:
            assert content is not None
            updated_manifest = content.replace(
                b"target\taarch64-apple-darwin\n",
                b"target\tx86_64-apple-darwin\n",
            )
            members[index] = (member, updated_manifest)
    if not updated_manifest:
        raise AssertionError("fixture manifest target was not found")
    manifest_sha = hashlib.sha256(updated_manifest).hexdigest()
    for index, (member, content) in enumerate(members):
        if member.name == sums_name:
            assert content is not None
            lines = content.decode("utf-8").splitlines()
            lines = [
                f"{manifest_sha}  manifest.tsv" if line.endswith("  manifest.tsv") else line
                for line in lines
            ]
            members[index] = (member, ("\n".join(lines) + "\n").encode("utf-8"))
    write_archive(path, members)


def assemble(dist: Path, *, check: bool = True) -> subprocess.CompletedProcess[str]:
    return run(
        RELEASE_TOOL,
        "assemble",
        "--dist-dir",
        dist,
        "--version",
        VERSION,
        "--revision",
        REVISION,
        "--tree",
        TREE,
        "--created",
        "2026-07-29T00:00:00Z",
        check=check,
    )


def verify(dist: Path, *, check: bool = True) -> subprocess.CompletedProcess[str]:
    return run(
        RELEASE_TOOL,
        "verify",
        "--dist-dir",
        dist,
        "--version",
        VERSION,
        "--revision",
        REVISION,
        "--tree",
        TREE,
        check=check,
    )


def update_top_sum(dist: Path, name: str) -> None:
    sums_path = dist / "SHA256SUMS"
    lines = sums_path.read_text(encoding="utf-8").splitlines()
    replacement = f"{sha256(dist / name)}  {name}"
    lines = [replacement if line.endswith(f"  {name}") else line for line in lines]
    sums_path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def assert_rejected(result: subprocess.CompletedProcess[str], expected: str) -> None:
    if result.returncode == 0:
        raise AssertionError(f"negative release case unexpectedly passed: {expected}")
    if expected not in result.stderr:
        raise AssertionError(
            f"negative release case did not report {expected!r}: {result.stderr!r}"
        )


def main() -> None:
    workflow_dir = REPO_ROOT / ".github" / "workflows"
    workflow = (workflow_dir / "release.yml").read_text(
        encoding="utf-8"
    )
    for required_fragment in (
        "workflow_dispatch:",
        "permissions: {}",
        "cancel-in-progress: false",
        "macos-14",
        "macos-15-intel",
        "ubuntu-22.04-arm",
        "ubuntu-22.04",
        "actions/attest@v4",
        'all(.targets[]; .source_mode == "locked-offline-source")',
        'gh release verify "$TAG"',
        "--draft",
        "--draft=false",
        "jq -r '.immutable'",
        "actions/runs?head_sha=$RELEASE_SHA",
        'path == ".github/workflows/ci.yml"',
        "actions/runs/$ci_run_id/jobs",
        "dse-installer.sh",
        "dist-manifest.json",
        "SBOM.spdx.json",
    ):
        if required_fragment not in workflow:
            raise AssertionError(
                f"release workflow is missing contract fragment: {required_fragment}"
            )
    for forbidden_fragment in (
        "actions/upload-artifact",
        "actions/download-artifact",
        "raw.githubusercontent.com",
        "--clobber",
        "check-runs?",
        "repos/$DSE_RELEASE_REPOSITORY/immutable-releases",
        "pull_request_target:",
    ):
        if forbidden_fragment in workflow:
            raise AssertionError(
                f"release workflow contains forbidden path: {forbidden_fragment}"
            )
    all_workflows = "\n".join(
        path.read_text(encoding="utf-8")
        for path in sorted(workflow_dir.glob("*.yml"))
    )
    if "actions/upload-artifact" in all_workflows or "actions/download-artifact" in all_workflows:
        raise AssertionError("Actions artifacts must not remain a DSE binary source")

    with tempfile.TemporaryDirectory(prefix="dse-release-test.") as temporary:
        root = Path(temporary)
        binaries = fixture_binaries(root / "bin")
        base = root / "base"
        build_archives(base, binaries)

        success = root / "success"
        copy_inputs(base, success)
        assemble(success)
        verify(success)
        checksummed_assets = {
            "dse-installer.sh",
            "dist-manifest.json",
            "SBOM.spdx.json",
            *(f"dse-{VERSION}-{target}.tar.gz" for target in TARGETS),
        }
        top_names = {
            line.split("  ", 1)[1]
            for line in (success / "SHA256SUMS").read_text(encoding="utf-8").splitlines()
        }
        if top_names != checksummed_assets:
            raise AssertionError("top-level release asset set is not canonical")

        symlink_case = root / "symlink"
        copy_inputs(base, symlink_case)
        mutate_symlink(symlink_case / f"dse-{VERSION}-{TARGETS[0]}.tar.gz")
        assert_rejected(assemble(symlink_case, check=False), "link or non-regular")

        traversal_case = root / "traversal"
        copy_inputs(base, traversal_case)
        mutate_traversal(traversal_case / f"dse-{VERSION}-{TARGETS[0]}.tar.gz")
        assert_rejected(assemble(traversal_case, check=False), "unsafe archive path")

        wrong_target_case = root / "wrong-target"
        copy_inputs(base, wrong_target_case)
        mutate_wrong_target(
            wrong_target_case / f"dse-{VERSION}-{TARGETS[0]}.tar.gz"
        )
        assert_rejected(
            assemble(wrong_target_case, check=False), "manifest target mismatch"
        )

        semantic_manifest_case = root / "semantic-manifest"
        shutil.copytree(success, semantic_manifest_case)
        manifest_path = semantic_manifest_case / "dist-manifest.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["targets"][0]["asset"] = "dse-foreign.tar.gz"
        manifest_path.write_text(json.dumps(manifest) + "\n", encoding="utf-8")
        update_top_sum(semantic_manifest_case, "dist-manifest.json")
        assert_rejected(
            verify(semantic_manifest_case, check=False), "manifest aarch64-apple-darwin asset mismatch"
        )

        incomplete_manifest_case = root / "incomplete-manifest"
        shutil.copytree(success, incomplete_manifest_case)
        manifest_path = incomplete_manifest_case / "dist-manifest.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        del manifest["windows_entry"]
        manifest_path.write_text(json.dumps(manifest) + "\n", encoding="utf-8")
        update_top_sum(incomplete_manifest_case, "dist-manifest.json")
        assert_rejected(
            verify(incomplete_manifest_case, check=False),
            "release manifest key set mismatch",
        )

        sbom_case = root / "sbom"
        shutil.copytree(success, sbom_case)
        sbom_path = sbom_case / "SBOM.spdx.json"
        sbom = json.loads(sbom_path.read_text(encoding="utf-8"))
        sbom["packages"] = []
        sbom_path.write_text(json.dumps(sbom) + "\n", encoding="utf-8")
        update_top_sum(sbom_case, "SBOM.spdx.json")
        assert_rejected(verify(sbom_case, check=False), "SBOM does not enumerate")

        sbom_identity_case = root / "sbom-identity"
        shutil.copytree(success, sbom_identity_case)
        sbom_path = sbom_identity_case / "SBOM.spdx.json"
        sbom = json.loads(sbom_path.read_text(encoding="utf-8"))
        sbom["documentNamespace"] = "https://example.invalid/foreign-sbom"
        sbom_path.write_text(json.dumps(sbom) + "\n", encoding="utf-8")
        update_top_sum(sbom_identity_case, "SBOM.spdx.json")
        assert_rejected(
            verify(sbom_identity_case, check=False), "SBOM release namespace mismatch"
        )

        installer_case = root / "installer"
        shutil.copytree(success, installer_case)
        installer_path = installer_case / "dse-installer.sh"
        installer_path.write_bytes(installer_path.read_bytes() + b"# tampered\n")
        update_top_sum(installer_case, "dse-installer.sh")
        assert_rejected(
            verify(installer_case, check=False), "payload terminator mismatch"
        )

    print("release self-test: PASS (asset identity + tamper matrix)")


if __name__ == "__main__":
    main()
