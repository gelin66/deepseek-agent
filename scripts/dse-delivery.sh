#!/usr/bin/env bash
set -euo pipefail

readonly PROGRAM_NAME="${0##*/}"
readonly DELIVERY_SCHEMA="dse.delivery.v1"
readonly PRODUCT_NAME="DSE"
readonly BINARIES="dse,dse-tui"

die() {
  printf '%s: %s\n' "$PROGRAM_NAME" "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
DSE local delivery owner

Usage:
  scripts/dse-delivery.sh package [options]
  scripts/dse-delivery.sh install --artifact FILE [--checksum FILE] [--prefix DIR] [--smoke-test]
  scripts/dse-delivery.sh verify [--prefix DIR]
  scripts/dse-delivery.sh rollback [--prefix DIR]
  scripts/dse-delivery.sh uninstall [--prefix DIR]
  scripts/dse-delivery.sh host-target

Package options:
  --output-dir DIR    Artifact destination (default: dist)
  --binary-dir DIR    Package prebuilt fixture/release binaries instead of building
  --version VERSION   Required with --binary-dir; otherwise workspace version
  --revision SHA      Required with --binary-dir; otherwise clean Git HEAD
  --source-tree SHA   Required with --binary-dir; otherwise Git HEAD tree
  --target TRIPLE     Required with --binary-dir; otherwise rustc host target
  --release-asset     Use stable dse-VERSION-TARGET.tar.gz outer asset naming

Install options:
  --artifact FILE     Local .tar.gz package
  --checksum FILE     Archive checksum sidecar (default: FILE.sha256)
  --prefix DIR        Absolute install prefix (default: /usr/local)
  --smoke-test        Run staged version and `dse doctor --json` before activation

The delivery commands perform no network requests. Packaging a source checkout
uses `cargo build --release --locked --offline`.
EOF
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

sha256_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1}'
  else
    die "sha256sum or shasum is required"
  fi
}

host_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os:$arch" in
    Darwin:arm64 | Darwin:aarch64)
      printf '%s\n' "aarch64-apple-darwin"
      ;;
    Darwin:x86_64)
      printf '%s\n' "x86_64-apple-darwin"
      ;;
    Linux:aarch64 | Linux:arm64)
      printf '%s\n' "aarch64-unknown-linux-gnu"
      ;;
    Linux:x86_64)
      printf '%s\n' "x86_64-unknown-linux-gnu"
      ;;
    *)
      die "unsupported delivery host: $os $arch"
      ;;
  esac
}

pinned_rust_channel() {
  awk '
    /^\[toolchain\]$/ { in_toolchain = 1; next }
    /^\[/ && in_toolchain { exit }
    in_toolchain && /^channel[[:space:]]*=/ {
      value = $0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/".*$/, "", value)
      print value
      exit
    }
  ' rust-toolchain.toml
}

select_installed_pinned_toolchain() {
  local expected candidate version_output actual_version installed_toolchains
  expected="$(pinned_rust_channel)"
  [ -n "$expected" ] || die "rust-toolchain.toml has no pinned channel"
  if command -v rustup >/dev/null 2>&1; then
    installed_toolchains="$(cd "${TMPDIR:-/tmp}" && rustup toolchain list)"
    candidate="$(
      printf '%s\n' "$installed_toolchains" |
        awk -v expected="$expected" '
          !found && ($1 == expected || index($1, expected "-") == 1) { found = $1 }
          END { if (found) print found }
        '
    )"
    if [ -z "$candidate" ] &&
      version_output="$(rustup run stable rustc --version 2>/dev/null)" &&
      [ "$(printf '%s\n' "$version_output" | awk '{ print $2; exit }')" = "$expected" ]; then
      candidate="stable"
    fi
    [ -n "$candidate" ] ||
      die "pinned Rust $expected is not installed (network installation is never automatic)"
    version_output="$(rustup run "$candidate" rustc --version)"
    actual_version="$(printf '%s\n' "$version_output" | awk '{ print $2; exit }')"
    [ "$actual_version" = "$expected" ] ||
      die "installed toolchain $candidate is Rust $actual_version, expected $expected"
    export RUSTUP_TOOLCHAIN="$candidate"
  else
    require_command rustc
    version_output="$(rustc --version)"
    actual_version="$(printf '%s\n' "$version_output" | awk '{ print $2; exit }')"
    [ "$actual_version" = "$expected" ] ||
      die "installed rustc is $actual_version, expected $expected"
  fi
}

absolute_existing_file() {
  local path="$1"
  local parent base
  [ -f "$path" ] || die "file not found: $path"
  parent="$(cd "$(dirname "$path")" && pwd -P)"
  base="${path##*/}"
  printf '%s/%s\n' "$parent" "$base"
}

absolute_directory() {
  local path="$1"
  mkdir -p "$path"
  (cd "$path" && pwd -P)
}

absolute_existing_directory() {
  local path="$1"
  [ -d "$path" ] && [ ! -L "$path" ] || die "directory not found: $path"
  (cd "$path" && pwd -P)
}

validate_prefix() {
  local prefix="$1"
  case "$prefix" in
    /*) ;;
    *) die "install prefix must be absolute: $prefix" ;;
  esac
  [ "$prefix" != "/" ] || die "install prefix cannot be /"
  case "$prefix" in
    *$'\n'* | *$'\r'* | *$'\t'*) die "install prefix contains control whitespace" ;;
  esac
}

validate_identity_value() {
  local label="$1"
  local value="$2"
  local pattern="$3"
  [[ "$value" =~ $pattern ]] || die "invalid $label in delivery identity: $value"
}

manifest_value() {
  local manifest="$1"
  local key="$2"
  awk -F '\t' -v wanted="$key" '
    $1 == wanted {
      if (found) exit 2
      found = 1
      print substr($0, length($1) + 2)
    }
    END {
      if (!found) exit 1
    }
  ' "$manifest"
}

validate_manifest() {
  local manifest="$1"
  local schema product version target revision tree binaries cargo_lock_sha
  schema="$(manifest_value "$manifest" schema)" || die "manifest is missing schema"
  product="$(manifest_value "$manifest" product)" || die "manifest is missing product"
  version="$(manifest_value "$manifest" version)" || die "manifest is missing version"
  target="$(manifest_value "$manifest" target)" || die "manifest is missing target"
  revision="$(manifest_value "$manifest" source_revision)" ||
    die "manifest is missing source_revision"
  tree="$(manifest_value "$manifest" source_tree)" || die "manifest is missing source_tree"
  binaries="$(manifest_value "$manifest" binaries)" || die "manifest is missing binaries"
  cargo_lock_sha="$(manifest_value "$manifest" cargo_lock_sha256)" ||
    die "manifest is missing cargo_lock_sha256"

  [ "$schema" = "$DELIVERY_SCHEMA" ] || die "unsupported delivery schema: $schema"
  [ "$product" = "$PRODUCT_NAME" ] || die "unexpected product identity: $product"
  [ "$binaries" = "$BINARIES" ] || die "artifact binary set is not canonical: $binaries"
  validate_identity_value version "$version" '^[A-Za-z0-9][A-Za-z0-9.+_-]*$'
  validate_identity_value target "$target" '^[A-Za-z0-9][A-Za-z0-9._-]*$'
  validate_identity_value source_revision "$revision" '^[0-9a-f]{40}$'
  validate_identity_value source_tree "$tree" '^[0-9a-f]{40}$'
  validate_identity_value cargo_lock_sha256 "$cargo_lock_sha" '^[0-9a-f]{64}$'
}

workspace_version() {
  awk '
    /^\[workspace\.package\]$/ { in_package = 1; next }
    /^\[/ && in_package { exit }
    in_package && /^version[[:space:]]*=/ {
      value = $0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/".*$/, "", value)
      print value
      exit
    }
  ' Cargo.toml
}

write_internal_checksums() {
  local root="$1"
  (
    cd "$root"
    printf '%s  %s\n' "$(sha256_file manifest.tsv)" "manifest.tsv"
    printf '%s  %s\n' "$(sha256_file LICENSE)" "LICENSE"
    printf '%s  %s\n' "$(sha256_file bin/dse)" "bin/dse"
    printf '%s  %s\n' "$(sha256_file bin/dse-tui)" "bin/dse-tui"
  ) >"$root/SHA256SUMS"
}

verify_internal_checksums() {
  local root="$1"
  local sums="$root/SHA256SUMS"
  local expected_path expected_hash actual_hash count
  [ -f "$sums" ] && [ ! -L "$sums" ] || die "artifact is missing regular SHA256SUMS"
  count=0
  while IFS='  ' read -r expected_hash expected_path extra; do
    [ -z "${extra:-}" ] || die "malformed SHA256SUMS record"
    case "$expected_path" in
      manifest.tsv | LICENSE | bin/dse | bin/dse-tui) ;;
      *) die "unexpected checksum path: $expected_path" ;;
    esac
    validate_identity_value checksum "$expected_hash" '^[0-9a-f]{64}$'
    [ -f "$root/$expected_path" ] && [ ! -L "$root/$expected_path" ] ||
      die "checksummed artifact file is missing or not regular: $expected_path"
    actual_hash="$(sha256_file "$root/$expected_path")"
    [ "$actual_hash" = "$expected_hash" ] ||
      die "artifact file checksum mismatch: $expected_path"
    count=$((count + 1))
  done <"$sums"
  [ "$count" -eq 4 ] || die "SHA256SUMS must contain exactly four canonical files"
}

package_command() {
  local output_dir="dist"
  local binary_dir=""
  local version=""
  local revision=""
  local source_tree=""
  local target=""
  local release_asset=0
  local repo_root stage_root package_root package_name archive archive_tmp
  local cargo_lock_sha rustc_line rustc_verbose source_mode

  while [ "$#" -gt 0 ]; do
    case "$1" in
      --output-dir)
        [ "$#" -ge 2 ] || die "--output-dir requires a value"
        output_dir="$2"
        shift 2
        ;;
      --binary-dir)
        [ "$#" -ge 2 ] || die "--binary-dir requires a value"
        binary_dir="$2"
        shift 2
        ;;
      --version)
        [ "$#" -ge 2 ] || die "--version requires a value"
        version="$2"
        shift 2
        ;;
      --revision)
        [ "$#" -ge 2 ] || die "--revision requires a value"
        revision="$2"
        shift 2
        ;;
      --source-tree)
        [ "$#" -ge 2 ] || die "--source-tree requires a value"
        source_tree="$2"
        shift 2
        ;;
      --target)
        [ "$#" -ge 2 ] || die "--target requires a value"
        target="$2"
        shift 2
        ;;
      --release-asset)
        release_asset=1
        shift
        ;;
      -h | --help)
        usage
        exit 0
        ;;
      *)
        die "unknown package option: $1"
        ;;
    esac
  done

  require_command awk
  require_command gzip
  require_command tar
  repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
  cd "$repo_root"
  [ -f Cargo.lock ] && [ -f LICENSE ] || die "package must run from a DSE source tree"
  cargo_lock_sha="$(sha256_file Cargo.lock)"

  if [ -n "$binary_dir" ]; then
    [ -n "$version" ] && [ -n "$revision" ] && [ -n "$source_tree" ] && [ -n "$target" ] ||
      die "--binary-dir requires --version, --revision, --source-tree, and --target"
    binary_dir="$(absolute_existing_directory "$binary_dir")"
    source_mode="prebuilt"
    rustc_line="prebuilt fixture or externally verified release binaries"
  else
    require_command cargo
    require_command git
    require_command rustc
    select_installed_pinned_toolchain
    [ -z "$(git status --porcelain=v1)" ] ||
      die "source package requires a clean Git worktree"
    version="${version:-$(workspace_version)}"
    revision="${revision:-$(git rev-parse HEAD)}"
    source_tree="${source_tree:-$(git rev-parse HEAD^{tree})}"
    rustc_verbose="$(rustc -vV)"
    target="$(
      if [ -n "$target" ]; then
        printf '%s\n' "$target"
      else
        printf '%s\n' "$rustc_verbose" |
          awk '$1 == "host:" && !found { found = $2 } END { print found }'
      fi
    )"
    [ "$target" = "$(host_target)" ] ||
      die "cross-target packaging is not supported by the local delivery owner"
    source_mode="locked-offline-source"
    rustc_line="$(rustc --version | tr '\t\r\n' '   ')"
    export CARGO_INCREMENTAL=0
    export CARGO_NET_OFFLINE=true
    export DSE_BUILD_SHA="$revision"
    : "${CARGO_TARGET_DIR:=${TMPDIR:-/tmp}/dse-delivery-target}"
    export CARGO_TARGET_DIR
    cargo build --release --locked --offline -p dse-cli -p dse-tui
    binary_dir="$CARGO_TARGET_DIR/release"
  fi

  validate_identity_value version "$version" '^[A-Za-z0-9][A-Za-z0-9.+_-]*$'
  validate_identity_value target "$target" '^[A-Za-z0-9][A-Za-z0-9._-]*$'
  validate_identity_value source_revision "$revision" '^[0-9a-f]{40}$'
  validate_identity_value source_tree "$source_tree" '^[0-9a-f]{40}$'
  for binary in dse dse-tui; do
    [ -f "$binary_dir/$binary" ] && [ ! -L "$binary_dir/$binary" ] ||
      die "canonical binary missing from $binary_dir: $binary"
    [ -x "$binary_dir/$binary" ] || die "canonical binary is not executable: $binary"
  done

  output_dir="$(absolute_directory "$output_dir")"
  stage_root="$(mktemp -d "${TMPDIR:-/tmp}/dse-package.XXXXXX")"
  trap "rm -rf '$stage_root'" EXIT
  package_name="dse-${version}-${target}-${revision:0:12}"
  package_root="$stage_root/$package_name"
  mkdir -p "$package_root/bin"
  cp "$binary_dir/dse" "$package_root/bin/dse"
  cp "$binary_dir/dse-tui" "$package_root/bin/dse-tui"
  cp LICENSE "$package_root/LICENSE"
  chmod 0755 "$package_root/bin/dse" "$package_root/bin/dse-tui"
  chmod 0644 "$package_root/LICENSE"
  {
    printf 'schema\t%s\n' "$DELIVERY_SCHEMA"
    printf 'product\t%s\n' "$PRODUCT_NAME"
    printf 'version\t%s\n' "$version"
    printf 'target\t%s\n' "$target"
    printf 'source_revision\t%s\n' "$revision"
    printf 'source_tree\t%s\n' "$source_tree"
    printf 'cargo_lock_sha256\t%s\n' "$cargo_lock_sha"
    printf 'rustc\t%s\n' "$rustc_line"
    printf 'source_mode\t%s\n' "$source_mode"
    printf 'binaries\t%s\n' "$BINARIES"
  } >"$package_root/manifest.tsv"
  chmod 0644 "$package_root/manifest.tsv"
  validate_manifest "$package_root/manifest.tsv"
  write_internal_checksums "$package_root"
  chmod 0644 "$package_root/SHA256SUMS"

  # Normalize file times and suppress AppleDouble/xattr sidecars. The package
  # file order is explicit, making repeated packaging deterministic per target.
  touch -t 198001010000 \
    "$package_root" \
    "$package_root/bin" \
    "$package_root/bin/dse" \
    "$package_root/bin/dse-tui" \
    "$package_root/LICENSE" \
    "$package_root/manifest.tsv" \
    "$package_root/SHA256SUMS"
  if [ "$release_asset" -eq 1 ]; then
    archive="$output_dir/dse-${version}-${target}.tar.gz"
  else
    archive="$output_dir/$package_name.tar.gz"
  fi
  [ ! -e "$archive" ] && [ ! -L "$archive" ] &&
    [ ! -e "$archive.sha256" ] && [ ! -L "$archive.sha256" ] ||
    die "refusing to overwrite an existing delivery artifact: $archive"
  archive_tmp="$archive.tmp.$$"
  (
    cd "$stage_root"
    COPYFILE_DISABLE=1 tar -cf - \
      "$package_name/manifest.tsv" \
      "$package_name/LICENSE" \
      "$package_name/SHA256SUMS" \
      "$package_name/bin/dse" \
      "$package_name/bin/dse-tui" \
      | gzip -n >"$archive_tmp"
  )
  mv "$archive_tmp" "$archive"
  printf '%s  %s\n' "$(sha256_file "$archive")" "${archive##*/}" >"$archive.sha256"
  chmod 0644 "$archive" "$archive.sha256"
  printf '%s\n' "$archive"
}

verify_archive_checksum() {
  local artifact="$1"
  local checksum="$2"
  local expected_hash expected_name extra actual_hash
  [ -f "$checksum" ] && [ ! -L "$checksum" ] || die "checksum sidecar not found: $checksum"
  IFS='  ' read -r expected_hash expected_name extra <"$checksum" ||
    die "failed to read checksum sidecar"
  [ -z "${extra:-}" ] || die "checksum sidecar must contain one record"
  validate_identity_value archive_checksum "$expected_hash" '^[0-9a-f]{64}$'
  [ "$expected_name" = "${artifact##*/}" ] ||
    die "checksum sidecar names $expected_name, expected ${artifact##*/}"
  [ "$(wc -l <"$checksum" | tr -d ' ')" -eq 1 ] ||
    die "checksum sidecar must contain exactly one record"
  actual_hash="$(sha256_file "$artifact")"
  [ "$actual_hash" = "$expected_hash" ] || die "archive checksum mismatch"
}

archive_root_name() {
  local artifact="$1"
  local entry root count
  root=""
  count=0
  while IFS= read -r entry; do
    [ -n "$entry" ] || die "archive contains an empty path"
    case "$entry" in
      /* | *"/../"* | "../"* | *"/.." | "." | "..")
        die "archive contains an unsafe path: $entry"
        ;;
    esac
    if [ -z "$root" ]; then
      root="${entry%%/*}"
      validate_identity_value archive_root "$root" '^dse-[A-Za-z0-9.+_-]+-[A-Za-z0-9._-]+-[0-9a-f]{12}$'
    fi
    case "$entry" in
      "$root/" | "$root/bin/" | "$root/manifest.tsv" | "$root/LICENSE" | \
        "$root/SHA256SUMS" | "$root/bin/dse" | "$root/bin/dse-tui")
        ;;
      *)
        die "archive contains a non-canonical entry: $entry"
        ;;
    esac
    count=$((count + 1))
  done < <(tar -tzf "$artifact")
  [ "$count" -eq 5 ] || die "archive must contain exactly five canonical entries"
  printf '%s\n' "$root"
}

atomic_symlink() {
  local target="$1"
  local link="$2"
  local temporary="$link.tmp.$$"
  [ ! -e "$temporary" ] && [ ! -L "$temporary" ] ||
    die "temporary activation link already exists: $temporary"
  ln -s "$target" "$temporary"
  case "$(uname -s)" in
    Darwin) mv -fh "$temporary" "$link" ;;
    Linux) mv -Tf "$temporary" "$link" ;;
    *) die "unsupported host for atomic activation" ;;
  esac
}

validate_delivery_link_slot() {
  local link="$1"
  if [ -e "$link" ] && [ ! -L "$link" ]; then
    die "refusing to replace non-symlink delivery path: $link"
  fi
}

validate_bin_slot() {
  local link="$1"
  local expected="$2"
  if [ -e "$link" ] || [ -L "$link" ]; then
    [ -L "$link" ] || die "refusing to replace existing program: $link"
    [ "$(readlink "$link")" = "$expected" ] ||
      die "refusing to replace foreign program symlink: $link"
  fi
}

verify_release_root() {
  local root="$1"
  local expected_target="$2"
  local manifest="$root/manifest.tsv"
  [ -f "$manifest" ] && [ ! -L "$manifest" ] || die "installed release has no manifest"
  validate_manifest "$manifest"
  [ "$(manifest_value "$manifest" target)" = "$expected_target" ] ||
    die "installed release target does not match this host"
  verify_internal_checksums "$root"
  [ -x "$root/bin/dse" ] && [ -x "$root/bin/dse-tui" ] ||
    die "installed release binaries are not executable"
}

parse_prefix_and_artifact() {
  local mode="$1"
  shift
  PARSED_PREFIX="/usr/local"
  PARSED_ARTIFACT=""
  PARSED_CHECKSUM=""
  PARSED_SMOKE_TEST=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --prefix)
        [ "$#" -ge 2 ] || die "--prefix requires a value"
        PARSED_PREFIX="$2"
        shift 2
        ;;
      --artifact)
        [ "$mode" = "install" ] || die "--artifact is only valid for install"
        [ "$#" -ge 2 ] || die "--artifact requires a value"
        PARSED_ARTIFACT="$2"
        shift 2
        ;;
      --checksum)
        [ "$mode" = "install" ] || die "--checksum is only valid for install"
        [ "$#" -ge 2 ] || die "--checksum requires a value"
        PARSED_CHECKSUM="$2"
        shift 2
        ;;
      --smoke-test)
        [ "$mode" = "install" ] || die "--smoke-test is only valid for install"
        PARSED_SMOKE_TEST=1
        shift
        ;;
      -h | --help)
        usage
        exit 0
        ;;
      *)
        die "unknown $mode option: $1"
        ;;
    esac
  done
  validate_prefix "$PARSED_PREFIX"
}

extract_verified_artifact() {
  local artifact="$1"
  local stage="$2"
  local root_name="$3"
  local target="$4"
  local smoke_test="$5"
  local extracted manifest version version_output
  COPYFILE_DISABLE=1 tar -xzf "$artifact" -C "$stage"
  extracted="$stage/$root_name"
  [ -d "$extracted" ] && [ ! -L "$extracted" ] || die "archive root is not a directory"
  manifest="$extracted/manifest.tsv"
  validate_manifest "$manifest"
  [ "$(manifest_value "$manifest" target)" = "$target" ] ||
    die "artifact target $(manifest_value "$manifest" target) does not match host $target"
  verify_internal_checksums "$extracted"
  [ -x "$extracted/bin/dse" ] && [ -x "$extracted/bin/dse-tui" ] ||
    die "artifact binaries are not executable"
  if [ "$smoke_test" -eq 1 ]; then
    version="$(manifest_value "$manifest" version)"
    version_output="$("$extracted/bin/dse" --version)" ||
      die "staged dse --version smoke failed"
    case "$version_output" in
      "dse $version"*) ;;
      *) die "staged dse version does not match artifact manifest" ;;
    esac
    version_output="$("$extracted/bin/dse-tui" --version)" ||
      die "staged dse-tui --version smoke failed"
    case "$version_output" in
      "dse-tui $version"*) ;;
      *) die "staged dse-tui version does not match artifact manifest" ;;
    esac
    "$extracted/bin/dse" doctor --json >/dev/null ||
      die "staged dse doctor --json smoke failed"
  fi
  VERIFIED_EXTRACTED="$extracted"
}

install_command() {
  parse_prefix_and_artifact install "$@"
  local prefix="$PARSED_PREFIX"
  local artifact checksum target delivery_root releases_root stage root_name extracted
  local preflight_stage
  local manifest version revision release_id destination current_target
  [ -n "$PARSED_ARTIFACT" ] || die "install requires --artifact"
  artifact="$(absolute_existing_file "$PARSED_ARTIFACT")"
  checksum="${PARSED_CHECKSUM:-$artifact.sha256}"
  checksum="$(absolute_existing_file "$checksum")"
  require_command tar
  target="$(host_target)"
  verify_archive_checksum "$artifact" "$checksum"
  root_name="$(archive_root_name "$artifact")"

  if [ "$PARSED_SMOKE_TEST" -eq 1 ]; then
    preflight_stage="$(mktemp -d "${TMPDIR:-/tmp}/dse-install-preflight.XXXXXX")"
    trap "rm -rf '$preflight_stage'" EXIT
    extract_verified_artifact "$artifact" "$preflight_stage" "$root_name" "$target" 1
    rm -rf "$preflight_stage"
    trap - EXIT
  fi

  delivery_root="$prefix/lib/dse"
  releases_root="$delivery_root/releases"
  validate_delivery_link_slot "$delivery_root/current"
  validate_delivery_link_slot "$delivery_root/previous"
  validate_bin_slot "$prefix/bin/dse" "../lib/dse/current/bin/dse"
  validate_bin_slot "$prefix/bin/dse-tui" "../lib/dse/current/bin/dse-tui"
  mkdir -p "$releases_root" "$prefix/bin"
  stage="$(mktemp -d "$delivery_root/.install.XXXXXX")"
  trap "rm -rf '$stage'" EXIT
  extract_verified_artifact "$artifact" "$stage" "$root_name" "$target" 0
  extracted="$VERIFIED_EXTRACTED"
  manifest="$extracted/manifest.tsv"
  version="$(manifest_value "$manifest" version)"
  revision="$(manifest_value "$manifest" source_revision)"
  release_id="${version}-${target}-${revision:0:12}"
  destination="$releases_root/$release_id"
  if [ -e "$destination" ]; then
    [ -d "$destination" ] && [ ! -L "$destination" ] ||
      die "immutable release path is not a directory: $destination"
    verify_release_root "$destination" "$target"
    [ "$(sha256_file "$destination/manifest.tsv")" = "$(sha256_file "$manifest")" ] ||
      die "immutable release identity already exists with different metadata"
  else
    mv "$extracted" "$destination"
  fi

  current_target=""
  if [ -L "$delivery_root/current" ]; then
    current_target="$(readlink "$delivery_root/current")"
    case "$current_target" in
      releases/*) ;;
      *) die "current activation link has an invalid target: $current_target" ;;
    esac
    [ -d "$delivery_root/$current_target" ] || die "current activation target is missing"
  fi
  if [ -n "$current_target" ] && [ "$current_target" != "releases/$release_id" ]; then
    atomic_symlink "$current_target" "$delivery_root/previous"
  fi
  atomic_symlink "releases/$release_id" "$delivery_root/current"
  atomic_symlink "../lib/dse/current/bin/dse" "$prefix/bin/dse"
  atomic_symlink "../lib/dse/current/bin/dse-tui" "$prefix/bin/dse-tui"
  verify_release_root "$destination" "$target"
  printf 'installed %s %s (%s)\n' "$PRODUCT_NAME" "$version" "${revision:0:12}"
}

verify_command() {
  parse_prefix_and_artifact verify "$@"
  local prefix="$PARSED_PREFIX"
  local delivery_root current_target root target
  delivery_root="$prefix/lib/dse"
  [ -L "$delivery_root/current" ] || die "DSE is not installed at $prefix"
  current_target="$(readlink "$delivery_root/current")"
  case "$current_target" in
    releases/*) ;;
    *) die "current activation link has an invalid target: $current_target" ;;
  esac
  root="$delivery_root/$current_target"
  target="$(host_target)"
  verify_release_root "$root" "$target"
  [ -L "$prefix/bin/dse" ] &&
    [ "$(readlink "$prefix/bin/dse")" = "../lib/dse/current/bin/dse" ] ||
    die "dse program link is missing or foreign"
  [ -L "$prefix/bin/dse-tui" ] &&
    [ "$(readlink "$prefix/bin/dse-tui")" = "../lib/dse/current/bin/dse-tui" ] ||
    die "dse-tui program link is missing or foreign"
  printf 'verified %s\n' "$(manifest_value "$root/manifest.tsv" version)"
}

rollback_command() {
  parse_prefix_and_artifact rollback "$@"
  local prefix="$PARSED_PREFIX"
  local delivery_root current_target previous_target target
  delivery_root="$prefix/lib/dse"
  [ -L "$delivery_root/current" ] || die "DSE is not installed at $prefix"
  [ -L "$delivery_root/previous" ] || die "no previous DSE release is available"
  current_target="$(readlink "$delivery_root/current")"
  previous_target="$(readlink "$delivery_root/previous")"
  case "$current_target:$previous_target" in
    releases/*:releases/*) ;;
    *) die "rollback links contain an invalid target" ;;
  esac
  [ "$current_target" != "$previous_target" ] || die "current and previous releases are identical"
  target="$(host_target)"
  verify_release_root "$delivery_root/$current_target" "$target"
  verify_release_root "$delivery_root/$previous_target" "$target"
  atomic_symlink "$previous_target" "$delivery_root/current"
  atomic_symlink "$current_target" "$delivery_root/previous"
  printf 'rolled back to %s\n' \
    "$(manifest_value "$delivery_root/$previous_target/manifest.tsv" version)"
}

uninstall_command() {
  parse_prefix_and_artifact uninstall "$@"
  local prefix="$PARSED_PREFIX"
  local delivery_root
  delivery_root="$prefix/lib/dse"
  validate_bin_slot "$prefix/bin/dse" "../lib/dse/current/bin/dse"
  validate_bin_slot "$prefix/bin/dse-tui" "../lib/dse/current/bin/dse-tui"
  if [ -L "$prefix/bin/dse" ]; then
    rm -f "$prefix/bin/dse"
  fi
  if [ -L "$prefix/bin/dse-tui" ]; then
    rm -f "$prefix/bin/dse-tui"
  fi
  if [ -e "$delivery_root" ] || [ -L "$delivery_root" ]; then
    [ -d "$delivery_root" ] && [ ! -L "$delivery_root" ] ||
      die "refusing to remove non-directory delivery root: $delivery_root"
    [ "$delivery_root" = "$prefix/lib/dse" ] ||
      die "resolved delivery root escaped the install prefix"
    [ -L "$delivery_root/current" ] ||
      die "refusing to remove an unowned delivery root without a current link"
    case "$(readlink "$delivery_root/current")" in
      releases/*) ;;
      *) die "refusing to remove a delivery root with a foreign current link" ;;
    esac
    rm -rf "$delivery_root"
  fi
  printf 'uninstalled %s programs; user data was preserved\n' "$PRODUCT_NAME"
}

main() {
  local command="${1:-}"
  if [ -z "$command" ]; then
    usage
    exit 2
  fi
  shift
  case "$command" in
    package) package_command "$@" ;;
    install) install_command "$@" ;;
    verify) verify_command "$@" ;;
    rollback) rollback_command "$@" ;;
    uninstall) uninstall_command "$@" ;;
    host-target)
      [ "$#" -eq 0 ] || die "host-target accepts no options"
      host_target
      ;;
    -h | --help | help)
      usage
      ;;
    *)
      die "unknown command: $command"
      ;;
  esac
}

main "$@"
