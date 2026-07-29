#!/usr/bin/env python3
"""Assemble and verify DSE's immutable GitHub Release asset set.

This is release-time tooling only. The public cold-install path is POSIX shell
and delegates all installation semantics to scripts/dse-delivery.sh.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import posixpath
import re
import subprocess
import sys
import tarfile
import tomllib
import urllib.parse
from pathlib import Path
from typing import Any


REPOSITORY = "gelin66/deepseek-agent"
RELEASE_SCHEMA = "dse.release.v1"
DELIVERY_SCHEMA = "dse.delivery.v1"
INSTALLER_NAME = "dse-installer.sh"
MANIFEST_NAME = "dist-manifest.json"
CHECKSUMS_NAME = "SHA256SUMS"
SBOM_NAME = "SBOM.spdx.json"
TARGETS = (
    (
        "aarch64-apple-darwin",
        "macos-14",
        "macOS 14 or later on Apple silicon",
    ),
    (
        "x86_64-apple-darwin",
        "macos-15-intel",
        "macOS 15 or later on Intel",
    ),
    (
        "aarch64-unknown-linux-gnu",
        "ubuntu-22.04-arm",
        "glibc 2.35 or later (verified on Ubuntu 22.04 arm64)",
    ),
    (
        "x86_64-unknown-linux-gnu",
        "ubuntu-22.04",
        "glibc 2.35 or later (Ubuntu 22.04 or WSL2 Ubuntu 22.04)",
    ),
)
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
VERSION_RE = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")


class ReleaseError(RuntimeError):
    pass


def fail(message: str) -> None:
    raise ReleaseError(message)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def validate_identity(version: str, revision: str, tree: str) -> None:
    if not VERSION_RE.fullmatch(version):
        fail(f"version must be an exact stable SemVer: {version}")
    if not SHA_RE.fullmatch(revision):
        fail(f"source revision must be a 40-character lowercase SHA: {revision}")
    if not SHA_RE.fullmatch(tree):
        fail(f"source tree must be a 40-character lowercase SHA: {tree}")


def normalized_created(value: str) -> str:
    try:
        parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        fail(f"created timestamp is not ISO-8601: {error}")
    if parsed.tzinfo is None:
        fail("created timestamp must include a timezone")
    return parsed.astimezone(dt.timezone.utc).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )


def parse_manifest_tsv(raw: bytes) -> dict[str, str]:
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as error:
        fail(f"delivery manifest is not UTF-8: {error}")
    result: dict[str, str] = {}
    for line in text.splitlines():
        key, separator, value = line.partition("\t")
        if not separator or not key or key in result:
            fail("delivery manifest contains a malformed or duplicate record")
        result[key] = value
    required = {
        "schema",
        "product",
        "version",
        "target",
        "source_revision",
        "source_tree",
        "cargo_lock_sha256",
        "rustc",
        "source_mode",
        "binaries",
    }
    if set(result) != required:
        fail(
            "delivery manifest key set mismatch: "
            f"expected {sorted(required)}, got {sorted(result)}"
        )
    return result


def parse_internal_sums(raw: bytes) -> dict[str, str]:
    try:
        lines = raw.decode("utf-8").splitlines()
    except UnicodeDecodeError as error:
        fail(f"inner SHA256SUMS is not UTF-8: {error}")
    result: dict[str, str] = {}
    for line in lines:
        digest, separator, name = line.partition("  ")
        if not separator or not SHA256_RE.fullmatch(digest) or name in result:
            fail("inner SHA256SUMS contains a malformed or duplicate record")
        result[name] = digest
    expected = {"manifest.tsv", "LICENSE", "bin/dse", "bin/dse-tui"}
    if set(result) != expected:
        fail("inner SHA256SUMS does not contain the canonical four-file set")
    return result


def inspect_archive(
    path: Path,
    *,
    version: str,
    target: str,
    revision: str,
    tree: str,
) -> dict[str, Any]:
    expected_name = f"dse-{version}-{target}.tar.gz"
    if path.name != expected_name or not path.is_file() or path.is_symlink():
        fail(f"release archive is missing, foreign, or not regular: {expected_name}")

    sidecar = path.with_name(f"{path.name}.sha256")
    outer_sha = sha256_file(path)
    if sidecar.exists():
        if not sidecar.is_file() or sidecar.is_symlink():
            fail(f"archive checksum sidecar is not regular: {sidecar.name}")
        lines = sidecar.read_text(encoding="utf-8").splitlines()
        if lines != [f"{outer_sha}  {path.name}"]:
            fail(f"archive checksum sidecar does not bind {path.name}")

    try:
        with tarfile.open(path, mode="r:gz") as archive:
            members = archive.getmembers()
            for member in members:
                normalized = posixpath.normpath(member.name)
                if (
                    member.name.startswith("/")
                    or normalized in {".", ".."}
                    or normalized.startswith("../")
                    or "/../" in f"/{member.name}/"
                    or normalized != member.name
                ):
                    fail(f"{path.name} contains an unsafe archive path: {member.name}")
            if len(members) != 5:
                fail(f"{path.name} must contain exactly five canonical files")
            roots = {member.name.split("/", 1)[0] for member in members}
            if len(roots) != 1:
                fail(f"{path.name} does not have one canonical archive root")
            root = roots.pop()
            root_pattern = re.compile(
                rf"^dse-{re.escape(version)}-{re.escape(target)}-{revision[:12]}$"
            )
            if not root_pattern.fullmatch(root):
                fail(f"{path.name} archive root does not bind release identity")
            expected_members = {
                f"{root}/manifest.tsv",
                f"{root}/LICENSE",
                f"{root}/SHA256SUMS",
                f"{root}/bin/dse",
                f"{root}/bin/dse-tui",
            }
            by_name = {member.name: member for member in members}
            if set(by_name) != expected_members:
                fail(f"{path.name} contains a non-canonical archive member")
            if any(not member.isfile() or member.issym() or member.islnk() for member in members):
                fail(f"{path.name} contains a link or non-regular member")
            for binary in (f"{root}/bin/dse", f"{root}/bin/dse-tui"):
                if by_name[binary].mode & 0o111 == 0:
                    fail(f"{path.name} contains a non-executable binary")

            contents: dict[str, bytes] = {}
            for member_name, member in by_name.items():
                extracted = archive.extractfile(member)
                if extracted is None:
                    fail(f"could not read {member_name} from {path.name}")
                contents[member_name.removeprefix(f"{root}/")] = extracted.read()
    except (tarfile.TarError, OSError) as error:
        fail(f"could not inspect {path.name}: {error}")

    manifest = parse_manifest_tsv(contents["manifest.tsv"])
    expected_manifest = {
        "schema": DELIVERY_SCHEMA,
        "product": "DSE",
        "version": version,
        "target": target,
        "source_revision": revision,
        "source_tree": tree,
        "binaries": "dse,dse-tui",
    }
    for key, expected in expected_manifest.items():
        if manifest[key] != expected:
            fail(f"{path.name} manifest {key} mismatch: {manifest[key]!r}")
    if not SHA256_RE.fullmatch(manifest["cargo_lock_sha256"]):
        fail(f"{path.name} has an invalid Cargo.lock identity")

    inner_sums = parse_internal_sums(contents["SHA256SUMS"])
    for member_name, expected_sha in inner_sums.items():
        if sha256_bytes(contents[member_name]) != expected_sha:
            fail(f"{path.name} inner checksum mismatch: {member_name}")

    return {
        "target": target,
        "asset": path.name,
        "sha256": outer_sha,
        "bytes": path.stat().st_size,
        "cargo_lock_sha256": manifest["cargo_lock_sha256"],
        "source_mode": manifest["source_mode"],
    }


def render_installer(repo_root: Path, output: Path) -> None:
    subprocess.run(
        [str(repo_root / "scripts" / "render-dse-installer.sh"), str(output)],
        cwd=repo_root,
        check=True,
    )
    subprocess.run(["/bin/sh", "-n", str(output)], check=True)
    rendered = output.read_bytes()
    marker = b"__DSE_DELIVERY_PAYLOAD_BELOW__\n"
    payload_end = b"__DSE_DELIVERY_PAYLOAD_END__\n"
    if rendered.count(marker) != 1:
        fail("rendered installer does not contain one delivery payload marker")
    prelude, tail = rendered.split(marker, 1)
    if not tail.endswith(payload_end) or tail.count(payload_end) != 1:
        fail("rendered installer does not contain one delivery payload terminator")
    payload = tail[: -len(payload_end)]
    delivery = (repo_root / "scripts" / "dse-delivery.sh").read_bytes()
    if payload != delivery:
        fail("rendered installer payload is not the canonical delivery owner")
    delivery_sha = sha256_bytes(delivery).encode("ascii")
    if b'DSE_DELIVERY_SHA256="' + delivery_sha + b'"' not in prelude:
        fail("rendered installer does not bind the canonical delivery SHA-256")


def spdx_id(name: str, version: str, index: int) -> str:
    safe = re.sub(r"[^A-Za-z0-9.-]", "-", f"{name}-{version}")
    return f"SPDXRef-Cargo-{safe}-{index}"


def cargo_download_location(package: dict[str, Any]) -> str:
    source = package.get("source")
    if not source:
        return "NOASSERTION"
    if source.startswith("registry+"):
        return (
            "https://crates.io/api/v1/crates/"
            f"{urllib.parse.quote(package['name'], safe='')}/"
            f"{urllib.parse.quote(package['version'], safe='')}/download"
        )
    if source.startswith("git+"):
        return source.removeprefix("git+").split("#", 1)[0]
    return "NOASSERTION"


def generate_sbom(
    repo_root: Path,
    *,
    version: str,
    tag: str,
    revision: str,
    created: str,
    output: Path,
) -> None:
    lock = tomllib.loads((repo_root / "Cargo.lock").read_text(encoding="utf-8"))
    cargo_packages: list[dict[str, Any]] = []
    relationships: list[dict[str, str]] = [
        {
            "spdxElementId": "SPDXRef-DOCUMENT",
            "relationshipType": "DESCRIBES",
            "relatedSpdxElement": "SPDXRef-Package-DSE",
        }
    ]
    for index, package in enumerate(
        sorted(
            lock.get("package", []),
            key=lambda item: (
                item["name"],
                item["version"],
                item.get("source", ""),
            ),
        ),
        start=1,
    ):
        identifier = spdx_id(package["name"], package["version"], index)
        purl = (
            "pkg:cargo/"
            f"{urllib.parse.quote(package['name'], safe='')}@"
            f"{urllib.parse.quote(package['version'], safe='')}"
        )
        entry: dict[str, Any] = {
            "name": package["name"],
            "SPDXID": identifier,
            "versionInfo": package["version"],
            "downloadLocation": cargo_download_location(package),
            "filesAnalyzed": False,
            "licenseConcluded": "NOASSERTION",
            "licenseDeclared": "NOASSERTION",
            "copyrightText": "NOASSERTION",
            "externalRefs": [
                {
                    "referenceCategory": "PACKAGE-MANAGER",
                    "referenceType": "purl",
                    "referenceLocator": purl,
                }
            ],
        }
        checksum = package.get("checksum")
        if checksum and SHA256_RE.fullmatch(checksum):
            entry["checksums"] = [
                {"algorithm": "SHA256", "checksumValue": checksum}
            ]
        cargo_packages.append(entry)
        relationships.append(
            {
                "spdxElementId": "SPDXRef-Package-DSE",
                "relationshipType": "DEPENDS_ON",
                "relatedSpdxElement": identifier,
            }
        )

    product_package = {
        "name": "DSE",
        "SPDXID": "SPDXRef-Package-DSE",
        "versionInfo": version,
        "downloadLocation": (
            f"https://github.com/{REPOSITORY}/releases/tag/{tag}"
        ),
        "filesAnalyzed": False,
        "licenseConcluded": "MIT",
        "licenseDeclared": "MIT",
        "copyrightText": "NOASSERTION",
        "supplier": "Person: gelin66",
        "externalRefs": [
            {
                "referenceCategory": "PACKAGE-MANAGER",
                "referenceType": "purl",
                "referenceLocator": f"pkg:github/{REPOSITORY}@{version}",
            }
        ],
    }
    document = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": f"DSE-{version}",
        "documentNamespace": (
            f"https://github.com/{REPOSITORY}/releases/download/{tag}/"
            f"{SBOM_NAME}#source-{revision}"
        ),
        "creationInfo": {
            "created": created,
            "creators": ["Tool: scripts/dse-release.py"],
        },
        "documentDescribes": ["SPDXRef-Package-DSE"],
        "packages": [product_package, *cargo_packages],
        "relationships": relationships,
    }
    output.write_text(
        json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )


def write_dist_manifest(output: Path, manifest: dict[str, Any]) -> None:
    targets = manifest.pop("targets")
    lines = ["{"]
    scalar_items = list(manifest.items())
    for key, value in scalar_items:
        encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
        lines.append(f"  {json.dumps(key)}: {encoded},")
    lines.append('  "targets": [')
    for index, target in enumerate(targets):
        suffix = "," if index + 1 < len(targets) else ""
        encoded = json.dumps(target, ensure_ascii=False, separators=(",", ":"))
        lines.append(f"    {encoded}{suffix}")
    lines.extend(["  ]", "}"])
    output.write_text("\n".join(lines) + "\n", encoding="utf-8")


def canonical_asset_names(version: str) -> list[str]:
    archives = [f"dse-{version}-{target}.tar.gz" for target, _, _ in TARGETS]
    return [INSTALLER_NAME, *archives, MANIFEST_NAME, SBOM_NAME]


def parse_top_sums(path: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeDecodeError) as error:
        fail(f"could not read {CHECKSUMS_NAME}: {error}")
    for line in lines:
        digest, separator, name = line.partition("  ")
        if not separator or not SHA256_RE.fullmatch(digest) or name in result:
            fail(f"{CHECKSUMS_NAME} contains a malformed or duplicate record")
        if "/" in name or name in {".", ".."}:
            fail(f"{CHECKSUMS_NAME} contains a non-canonical asset path")
        result[name] = digest
    return result


def assemble(args: argparse.Namespace, repo_root: Path) -> None:
    version = args.version
    revision = args.revision
    tree = args.tree
    validate_identity(version, revision, tree)
    created = normalized_created(args.created)
    tag = f"v{version}"
    dist = args.dist_dir.resolve()
    if not dist.is_dir() or dist.is_symlink():
        fail(f"dist directory is missing or not regular: {dist}")

    for name in (INSTALLER_NAME, MANIFEST_NAME, CHECKSUMS_NAME, SBOM_NAME):
        path = dist / name
        if path.exists() or path.is_symlink():
            fail(f"refusing to overwrite release metadata: {path}")

    target_records: list[dict[str, Any]] = []
    cargo_lock_hashes: set[str] = set()
    for target, runner, minimum in TARGETS:
        record = inspect_archive(
            dist / f"dse-{version}-{target}.tar.gz",
            version=version,
            target=target,
            revision=revision,
            tree=tree,
        )
        cargo_lock_hashes.add(record.pop("cargo_lock_sha256"))
        record["verified_runner"] = runner
        record["minimum_platform"] = minimum
        target_records.append(record)
    expected_lock = sha256_file(repo_root / "Cargo.lock")
    if cargo_lock_hashes != {expected_lock}:
        fail("release archives do not bind the current Cargo.lock")

    render_installer(repo_root, dist / INSTALLER_NAME)
    generate_sbom(
        repo_root,
        version=version,
        tag=tag,
        revision=revision,
        created=created,
        output=dist / SBOM_NAME,
    )
    manifest = {
        "schema": RELEASE_SCHEMA,
        "product": "DSE",
        "repository": REPOSITORY,
        "version": version,
        "tag": tag,
        "source_revision": revision,
        "source_tree": tree,
        "created_at": created,
        "installer": INSTALLER_NAME,
        "checksums": CHECKSUMS_NAME,
        "sbom": SBOM_NAME,
        "cargo_lock_sha256": expected_lock,
        "windows_native": False,
        "windows_entry": "WSL2 Ubuntu 22.04 x86_64",
        "targets": target_records,
    }
    write_dist_manifest(dist / MANIFEST_NAME, manifest)

    with (dist / CHECKSUMS_NAME).open("w", encoding="utf-8", newline="\n") as sums:
        for name in canonical_asset_names(version):
            sums.write(f"{sha256_file(dist / name)}  {name}\n")

    verify_release(dist, version=version, revision=revision, tree=tree, repo_root=repo_root)
    print(dist)


def verify_release(
    dist: Path,
    *,
    version: str,
    revision: str,
    tree: str,
    repo_root: Path,
) -> None:
    validate_identity(version, revision, tree)
    if not dist.is_dir() or dist.is_symlink():
        fail(f"dist directory is missing or not regular: {dist}")
    expected_names = canonical_asset_names(version)
    sums = parse_top_sums(dist / CHECKSUMS_NAME)
    if set(sums) != set(expected_names):
        fail(
            f"{CHECKSUMS_NAME} asset set mismatch: expected {expected_names}, "
            f"got {sorted(sums)}"
        )
    for name in expected_names:
        path = dist / name
        if not path.is_file() or path.is_symlink():
            fail(f"release asset is missing or not regular: {name}")
        if sha256_file(path) != sums[name]:
            fail(f"top-level checksum mismatch: {name}")

    manifest = json.loads((dist / MANIFEST_NAME).read_text(encoding="utf-8"))
    expected_manifest_keys = {
        "schema",
        "product",
        "repository",
        "version",
        "tag",
        "source_revision",
        "source_tree",
        "created_at",
        "installer",
        "checksums",
        "sbom",
        "cargo_lock_sha256",
        "windows_native",
        "windows_entry",
        "targets",
    }
    if set(manifest) != expected_manifest_keys:
        fail("release manifest key set mismatch")
    expected_scalars = {
        "schema": RELEASE_SCHEMA,
        "product": "DSE",
        "repository": REPOSITORY,
        "version": version,
        "tag": f"v{version}",
        "source_revision": revision,
        "source_tree": tree,
        "installer": INSTALLER_NAME,
        "checksums": CHECKSUMS_NAME,
        "sbom": SBOM_NAME,
        "cargo_lock_sha256": sha256_file(repo_root / "Cargo.lock"),
        "windows_native": False,
        "windows_entry": "WSL2 Ubuntu 22.04 x86_64",
    }
    for key, expected in expected_scalars.items():
        if manifest.get(key) != expected:
            fail(f"release manifest {key} mismatch")
    created_at = manifest.get("created_at")
    if not isinstance(created_at, str) or normalized_created(created_at) != created_at:
        fail("release manifest created_at is not canonical UTC ISO-8601")
    target_records = manifest.get("targets")
    if not isinstance(target_records, list) or len(target_records) != len(TARGETS):
        fail("release manifest target count mismatch")
    by_target = {record.get("target"): record for record in target_records}
    for target, runner, minimum in TARGETS:
        record = by_target.get(target)
        if not isinstance(record, dict):
            fail(f"release manifest is missing target {target}")
        if set(record) != {
            "target",
            "asset",
            "sha256",
            "bytes",
            "source_mode",
            "verified_runner",
            "minimum_platform",
        }:
            fail(f"release manifest {target} key set mismatch")
        archive_record = inspect_archive(
            dist / f"dse-{version}-{target}.tar.gz",
            version=version,
            target=target,
            revision=revision,
            tree=tree,
        )
        for key in ("asset", "sha256", "bytes", "source_mode"):
            if record.get(key) != archive_record[key]:
                fail(f"release manifest {target} {key} mismatch")
        if record.get("verified_runner") != runner or record.get(
            "minimum_platform"
        ) != minimum:
            fail(f"release manifest {target} support evidence mismatch")

    installer = dist / INSTALLER_NAME
    rendered = installer.read_bytes()
    marker = b"__DSE_DELIVERY_PAYLOAD_BELOW__\n"
    payload_end = b"__DSE_DELIVERY_PAYLOAD_END__\n"
    if rendered.count(marker) != 1:
        fail("release installer payload marker mismatch")
    _, tail = rendered.split(marker, 1)
    if not tail.endswith(payload_end) or tail.count(payload_end) != 1:
        fail("release installer payload terminator mismatch")
    payload = tail[: -len(payload_end)]
    if payload != (repo_root / "scripts" / "dse-delivery.sh").read_bytes():
        fail("release installer does not contain the current delivery owner")
    subprocess.run(["/bin/sh", "-n", str(installer)], check=True)

    sbom = json.loads((dist / SBOM_NAME).read_text(encoding="utf-8"))
    if sbom.get("spdxVersion") != "SPDX-2.3" or sbom.get("SPDXID") != "SPDXRef-DOCUMENT":
        fail("SBOM is not an SPDX 2.3 document")
    packages = sbom.get("packages")
    if not isinstance(packages, list) or len(packages) < 2:
        fail("SBOM does not enumerate the DSE package and Cargo dependencies")
    if sbom.get("name") != f"DSE-{version}" or sbom.get("documentDescribes") != [
        "SPDXRef-Package-DSE"
    ]:
        fail("SBOM product identity mismatch")
    expected_namespace = (
        f"https://github.com/{REPOSITORY}/releases/download/v{version}/"
        f"{SBOM_NAME}#source-{revision}"
    )
    if sbom.get("documentNamespace") != expected_namespace:
        fail("SBOM release namespace mismatch")
    product_packages = [
        package
        for package in packages
        if isinstance(package, dict) and package.get("SPDXID") == "SPDXRef-Package-DSE"
    ]
    if len(product_packages) != 1 or product_packages[0].get("versionInfo") != version:
        fail("SBOM DSE package version mismatch")


def verify(args: argparse.Namespace, repo_root: Path) -> None:
    verify_release(
        args.dist_dir.resolve(),
        version=args.version,
        revision=args.revision,
        tree=args.tree,
        repo_root=repo_root,
    )
    print(args.dist_dir.resolve())


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    subcommands = root.add_subparsers(dest="command", required=True)
    for name in ("assemble", "verify"):
        command = subcommands.add_parser(name)
        command.add_argument("--dist-dir", type=Path, required=True)
        command.add_argument("--version", required=True)
        command.add_argument("--revision", required=True)
        command.add_argument("--tree", required=True)
        if name == "assemble":
            command.add_argument("--created", required=True)
    return root


def main() -> int:
    args = parser().parse_args()
    repo_root = Path(__file__).resolve().parent.parent
    if args.command == "assemble":
        assemble(args, repo_root)
    else:
        verify(args, repo_root)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ReleaseError, OSError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"dse-release: {error}", file=sys.stderr)
        raise SystemExit(1)
