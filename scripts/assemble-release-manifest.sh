#!/usr/bin/env bash
set -euo pipefail

version="$1"
release_dir="${2:-release}"

shopt -s nullglob
inputs=("${release_dir}"/manifest-target-*.json)
if (( ${#inputs[@]} == 0 )); then
  echo "no per-target release manifests found" >&2
  exit 2
fi

jq -s --arg version "$version" \
  '{version: $version, protocol: 1, targets: (map({key: .target, value: {path: .path, sha256: .sha256, sbom: .sbom}}) | from_entries)}' \
  "${inputs[@]}" > "${release_dir}/release-manifest.json"

jq --arg version "$version" \
  '{tool: "engr", tool_version: $version, protocol_version: "1", event_schema_version: 1, state_schema_version: 1, repository: "lukeo3o1/engr", artifacts: (.targets | with_entries(.value = {file: .value.path, sha256: .value.sha256, sbom: .value.sbom}))}' \
  "${release_dir}/release-manifest.json" > "${release_dir}/TOOLING.lock.json"
