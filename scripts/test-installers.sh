#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
installer="${repo_root}/install.sh"

bash -n "$installer"

test_root="$(mktemp -d "${TMPDIR:-/tmp}/engr-installer-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT
manifest="${test_root}/release-manifest.json"
cat > "$manifest" <<'EOF'
{
  "version": "0.1.0",
  "protocol": 1,
  "targets": {
    "x86_64-unknown-linux-musl": {
      "path": "engr-0.1.0-x86_64-unknown-linux-musl.tar.gz",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      "sbom": "sbom-x86_64-unknown-linux-musl.cdx.json"
    }
  }
}
EOF

# shellcheck source=/dev/null
source "$installer"
[[ "$(manifest_value "$manifest" version)" == '0.1.0' ]]
[[ "$(manifest_protocol "$manifest")" == '1' ]]
metadata="$(manifest_target_metadata "$manifest" 'x86_64-unknown-linux-musl')"
IFS=$'\t' read -r artifact sha256 sbom <<< "$metadata"
[[ "$artifact" == 'engr-0.1.0-x86_64-unknown-linux-musl.tar.gz' ]]
[[ "$sha256" == '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' ]]
[[ "$sbom" == 'sbom-x86_64-unknown-linux-musl.cdx.json' ]]
[[ "$(normalize_version 'v0.1.0')" == '0.1.0' ]]

[[ "$(
  uname() {
    case "$1" in
      -s) printf '%s\n' 'Darwin' ;;
      -m) printf '%s\n' 'arm64' ;;
    esac
  }
  detect_target
)" == 'aarch64-apple-darwin' ]]
if (
  uname() {
    case "$1" in
      -s) printf '%s\n' 'Darwin' ;;
      -m) printf '%s\n' 'x86_64' ;;
    esac
  }
  detect_target
) >"${test_root}/intel-macos-output" 2>&1; then
  printf 'installer accepted unsupported Intel macOS\n' >&2
  exit 1
fi
grep -q 'Apple Silicon macOS only' "${test_root}/intel-macos-output"

release_dir="${test_root}/release"
stage_dir="${test_root}/stage"
install_dir="${test_root}/bin"
mkdir -p "$release_dir" "$stage_dir"
cat > "${stage_dir}/engr" <<'EOF'
#!/usr/bin/env bash
if [[ "$1" == 'version' && "$2" == '--json' ]]; then
  printf '%s\n' '{'
  printf '%s\n' '  "implementation": "rust",'
  printf '%s\n' '  "implementation_version": "0.1.0"'
  printf '%s\n' '}'
  exit 0
fi
exit 2
EOF
chmod 0755 "${stage_dir}/engr"
archive_name='engr-0.1.0-x86_64-unknown-linux-musl.tar.gz'
tar -C "$stage_dir" -czf "${release_dir}/${archive_name}" engr
archive_hash="$(sha256sum "${release_dir}/${archive_name}" | awk '{print $1}')"
printf '%s  %s\n' "$archive_hash" "$archive_name" > "${release_dir}/${archive_name}.sha256"
cat > "${release_dir}/release-manifest.json" <<EOF
{
  "version": "0.1.0",
  "protocol": 1,
  "targets": {
    "x86_64-unknown-linux-musl": {
      "path": "${archive_name}",
      "sha256": "${archive_hash}",
      "sbom": "sbom-x86_64-unknown-linux-musl.cdx.json"
    }
  }
}
EOF

run_fixture_install() {
  local destination="$1"
  (
    download() {
      cp "${release_dir}/$(basename "$1")" "$2"
    }
    version='0.1.0'
    bin_dir="$destination"
    target='x86_64-unknown-linux-musl'
    main
  )
}

run_fixture_install "$install_dir"
[[ -x "${install_dir}/engr" ]]
"${install_dir}/engr" version --json | grep -Eq '"implementation_version"[[:space:]]*:[[:space:]]*"0.1.0"'

printf '%064d  %s\n' 0 "$archive_name" > "${release_dir}/${archive_name}.sha256"
if run_fixture_install "${test_root}/rejected-bin"; then
  printf 'installer accepted a checksum file that disagreed with the manifest\n' >&2
  exit 1
fi
[[ ! -e "${test_root}/rejected-bin/engr" ]]

printf 'Unix installer syntax, manifest parsing, checksum verification, rejection, and installation verified.\n'
