#!/usr/bin/env bash
set -Eeuo pipefail

temporary_directories=()
cleanup() {
  local path
  for path in "${temporary_directories[@]}"; do
    [[ -n "$path" && -d "$path" ]] || continue
    chmod -R u+w "$path" 2>/dev/null || true
    rm -rf -- "$path"
  done
}
trap cleanup EXIT

die() {
  printf 'Telegram Personal MCP release adapter: %s\n' "$*" >&2
  exit 1
}

require_tool() { command -v "$1" >/dev/null 2>&1 || die "required tool is unavailable: $1"; }
require_commit() { [[ "$1" =~ ^[0-9a-f]{40}$ ]] || die "invalid Git commit"; }
require_sha256() { [[ "$1" =~ ^[0-9a-f]{64}$ ]] || die "invalid SHA-256"; }
require_tag() { [[ "$1" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$ ]] || die "invalid release tag"; }
require_source_head() { [[ "$(git rev-parse HEAD)" == "$1" ]] || die "source HEAD changed"; }

create_source_snapshot() {
  local commit="$1" snapshot
  snapshot="$(mktemp -d "${TMPDIR:-/tmp}/telegram-personal-mcp-source.XXXXXX")"
  temporary_directories+=("$snapshot")
  git archive --format=tar "$commit" | tar -xf - -C "$snapshot" \
    || die "unable to materialize the exact source snapshot"
  source_snapshot="$snapshot"
}

parse_args() {
  commit=""; release_id=""; transaction_id=""; output=""; manifest=""; manifest_sha256=""
  tag=""; remote=""; repository=""; workflow=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --commit) commit="${2:-}"; shift 2 ;;
      --release-id) release_id="${2:-}"; shift 2 ;;
      --transaction-id) transaction_id="${2:-}"; shift 2 ;;
      --output) output="${2:-}"; shift 2 ;;
      --manifest) manifest="${2:-}"; shift 2 ;;
      --manifest-sha256) manifest_sha256="${2:-}"; shift 2 ;;
      --tag) tag="${2:-}"; shift 2 ;;
      --remote) remote="${2:-}"; shift 2 ;;
      --repository) repository="${2:-}"; shift 2 ;;
      --workflow) workflow="${2:-}"; shift 2 ;;
      *) die "unknown argument: $1" ;;
    esac
  done
  require_commit "$commit"
  [[ -n "$release_id" ]] || die "release id is required"
}

read_version() {
  cargo metadata --locked --no-deps --format-version 1 |
    jq -r '.packages[] | select(.name == "telegram-personal-mcp") | .version'
}

build_release() {
  parse_args "$@"
  [[ -n "$output" ]] || die "output path is required"
  require_source_head "$commit"
  for tool in cargo git jq sha256sum mktemp tar; do require_tool "$tool"; done
  local version output_dir temporary notes_file workflow_file notes_sha256 workflow_sha256 snapshot
  create_source_snapshot "$commit"
  snapshot="$source_snapshot"
  pushd "$snapshot" >/dev/null
  version="$(read_version)"
  tag="v$version"
  require_tag "$tag"
  notes_file=".github/releases/$tag.md"
  workflow_file=".github/workflows/release.yml"
  [[ -f "$notes_file" ]] || die "release notes are missing for $tag"
  [[ -f "$workflow_file" ]] || die "release workflow is missing"
  CARGO_TARGET_DIR="$snapshot/target" cargo fmt --all -- --check
  CARGO_TARGET_DIR="$snapshot/target" cargo test --workspace --all-targets --locked
  CARGO_TARGET_DIR="$snapshot/target" cargo clippy --workspace --all-targets --locked -- -D warnings
  CARGO_TARGET_DIR="$snapshot/target" cargo package --locked >/dev/null
  notes_sha256="$(sha256sum "$notes_file" | awk '{print $1}')"
  workflow_sha256="$(sha256sum "$workflow_file" | awk '{print $1}')"
  popd >/dev/null
  require_source_head "$commit"
  output_dir="$(dirname "$output")"
  mkdir -p "$output_dir"
  temporary="$output.tmp.$$"
  jq -n \
    --argjson schema_version 1 --arg project_id "telegram-personal-mcp-release" \
    --arg release_id "$release_id" --arg source_commit "$commit" --arg tag "$tag" \
    --arg version "$version" --arg notes_sha256 "$notes_sha256" --arg workflow_sha256 "$workflow_sha256" \
    '{schema_version:$schema_version,project_id:$project_id,release_id:$release_id,
      source_commit:$source_commit,tag:$tag,version:$version,
      notes_sha256:$notes_sha256,workflow_sha256:$workflow_sha256}' >"$temporary"
  chmod 0644 "$temporary"
  mv -f "$temporary" "$output"
  printf 'manifest=%s\nmanifest_sha256=%s\ntag=%s\n' \
    "$output" "$(sha256sum "$output" | awk '{print $1}')" "$tag"
}

verify_release() {
  parse_args "$@"
  require_tag "$tag"
  [[ -f "$manifest" && ! -L "$manifest" ]] || die "manifest is not a regular file"
  require_sha256 "$manifest_sha256"
  [[ "$(sha256sum "$manifest" | awk '{print $1}')" == "$manifest_sha256" ]] || die "manifest hash mismatch"
  for tool in cargo git gh jq sha256sum mktemp tar; do require_tool "$tool"; done
  jq -e --arg commit "$commit" --arg release_id "$release_id" --arg tag "$tag" \
    '.schema_version == 1 and .project_id == "telegram-personal-mcp-release"
     and .source_commit == $commit and .release_id == $release_id and .tag == $tag
     and (.notes_sha256 | test("^[0-9a-f]{64}$"))
     and (.workflow_sha256 | test("^[0-9a-f]{64}$"))' "$manifest" >/dev/null \
    || die "manifest does not match deployment binding"
  require_source_head "$commit"
  local snapshot local_tag remote_tag
  create_source_snapshot "$commit"
  snapshot="$source_snapshot"
  pushd "$snapshot" >/dev/null
  [[ "$(read_version)" == "${tag#v}" ]] || die "tag does not match Cargo package version"
  [[ "$(sha256sum ".github/releases/$tag.md" | awk '{print $1}')" == "$(jq -r .notes_sha256 "$manifest")" ]] || die "release notes changed"
  [[ "$(sha256sum ".github/workflows/$workflow" | awk '{print $1}')" == "$(jq -r .workflow_sha256 "$manifest")" ]] || die "release workflow changed"
  popd >/dev/null
  local_tag="$(git rev-parse "refs/tags/$tag^{commit}" 2>/dev/null || true)"
  [[ -z "$local_tag" || "$local_tag" == "$commit" ]] || die "local tag already points elsewhere"
  remote_tag="$(git ls-remote "$remote" "refs/tags/$tag^{}" | awk 'NR==1 {print $1}')"
  if [[ -z "$remote_tag" ]]; then
    remote_tag="$(git ls-remote "$remote" "refs/tags/$tag" | awk 'NR==1 {print $1}')"
  fi
  [[ -z "$remote_tag" || "$remote_tag" == "$commit" ]] || die "remote tag already points elsewhere"
}

apply_release() {
  parse_args "$@"
  [[ "$transaction_id" =~ ^[0-9a-f]{32}$ ]] || die "invalid transaction id"
  [[ "$remote" == "origin" ]] || die "unexpected Git remote"
  require_tool git; require_tool gh; require_tool jq
  local apply_tag="$tag" apply_commit="$commit" run_id run_json status="" conclusion="" release_json
  verify_release "$@"
  if [[ "$(git rev-parse "refs/tags/$apply_tag^{commit}" 2>/dev/null || true)" != "$apply_commit" ]]; then
    git tag -a "$apply_tag" "$apply_commit" -m "$apply_tag"
  fi
  git push "$remote" "refs/tags/$apply_tag"
  for _ in $(seq 1 60); do
    run_json="$(gh run list --repo "$repository" --workflow "$workflow" --branch "$apply_tag" --limit 20 \
      --json databaseId,headSha,status,conclusion | jq -c --arg commit "$apply_commit" \
      '[.[] | select(.headSha == $commit)] | sort_by(.databaseId) | last // empty')"
    if [[ -n "$run_json" ]]; then
      run_id="$(jq -r .databaseId <<<"$run_json")"
      status="$(jq -r .status <<<"$run_json")"
      conclusion="$(jq -r '.conclusion // ""' <<<"$run_json")"
      if [[ "$status" == "completed" ]]; then
        [[ "$conclusion" == "success" ]] || die "release workflow failed: run $run_id"
        break
      fi
    fi
    sleep 10
  done
  [[ "$status" == "completed" && "$conclusion" == "success" ]] || die "release workflow did not complete"
  release_json="$(gh release view "$apply_tag" --repo "$repository" --json tagName,isDraft,isPrerelease,url)"
  jq -e --arg tag "$apply_tag" '.tagName == $tag and .isDraft == false and (.url | length) > 0' <<<"$release_json" >/dev/null \
    || die "GitHub release readback is not the exact published release"
  printf '%s\n' "$release_json"
}

main() {
  local command_name="${1:-}"; [[ -n "$command_name" ]] || die "command is required"; shift
  case "$command_name" in
    build) build_release "$@" ;;
    verify) verify_release "$@" ;;
    apply) apply_release "$@" ;;
    *) die "unknown command: $command_name" ;;
  esac
}
main "$@"
