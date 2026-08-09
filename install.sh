#!/usr/bin/env bash
# Install a verified Engr release on Linux or macOS.
set -euo pipefail

readonly REPOSITORY="lukeo3o1/engr"
readonly DEFAULT_BIN_DIR="${HOME:+${HOME}/.local/bin}"

version="${ENGR_VERSION:-}"
bin_dir="${ENGR_INSTALL_DIR:-$DEFAULT_BIN_DIR}"
target="${ENGR_TARGET:-}"

usage() {
  cat <<'EOF'
Usage: install.sh [--version VERSION] [--bin-dir PATH] [--target TARGET]

Installs a verified Engr release for Linux or Apple Silicon macOS without sudo.

Options:
  --version VERSION  Release version to install (default: latest GitHub release)
  --bin-dir PATH     Destination directory (default: $ENGR_INSTALL_DIR or ~/.local/bin)
  --target TARGET    Exact release target to install. Linux defaults to musl for
                     portability; use this to select a GNU artifact explicitly.
  -h, --help         Show this help text

Environment equivalents: ENGR_VERSION, ENGR_INSTALL_DIR, and ENGR_TARGET.
EOF
}

die() {
  printf 'engr installer: %s\n' "$*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

download() {
  local url="$1"
  local destination="$2"
  curl --fail --location --silent --show-error --retry 3 --output "$destination" "$url"
}

normalize_version() {
  local candidate="$1"
  candidate="${candidate#v}"
  [[ "$candidate" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.+][0-9A-Za-z.-]+)?$ ]] \
    || die "invalid release version: $1"
  printf '%s\n' "$candidate"
}

latest_version() {
  local url
  url="$(curl --fail --location --silent --show-error --retry 3 --output /dev/null --write-out '%{url_effective}' "https://github.com/${REPOSITORY}/releases/latest")" \
    || die "could not resolve the latest GitHub release; pass --version explicitly"
  normalize_version "${url##*/}"
}

detect_target() {
  local os architecture prefix
  os="$(uname -s)"
  architecture="$(uname -m)"
  case "$os" in
    Darwin)
      case "$architecture" in
        arm64|aarch64) printf '%s\n' 'aarch64-apple-darwin' ;;
        x86_64|amd64) die 'Intel macOS is not supported; Engr releases support Apple Silicon macOS only' ;;
        *) die "unsupported macOS architecture: $architecture" ;;
      esac
      ;;
    Linux)
      case "$architecture" in
        x86_64|amd64) prefix='x86_64' ;;
        aarch64|arm64) prefix='aarch64' ;;
        *) die "unsupported Linux architecture: $architecture" ;;
      esac
      # musl is the portable default. Use --target for the GNU artifact.
      printf '%s\n' "${prefix}-unknown-linux-musl"
      ;;
    *) die "unsupported operating system: $os" ;;
  esac
}

manifest_value() {
  local manifest="$1"
  local key="$2"
  sed -n "s/^[[:space:]]*\"${key}\":[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p" "$manifest" | head -n 1
}

manifest_protocol() {
  sed -n 's/^[[:space:]]*"protocol":[[:space:]]*\([0-9][0-9]*\).*/\1/p' "$1" | head -n 1
}

manifest_target_metadata() {
  local manifest="$1"
  local wanted="$2"
  awk -F '"' -v wanted="$wanted" '
    index($0, "\"" wanted "\":") > 0 { inside = 1; next }
    inside && /"path"/ { path = $4 }
    inside && /"sha256"/ { sha256 = $4 }
    inside && /"sbom"/ { sbom = $4 }
    inside && path != "" && sha256 != "" && sbom != "" && $0 ~ /^[[:space:]]*}[,]?[[:space:]]*$/ {
      print path "\t" sha256 "\t" sbom
      exit
    }
  ' "$manifest"
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    die 'required command not found: sha256sum or shasum'
  fi
}

path_contains() {
  local directory="$1"
  local entry
  IFS=':' read -r -a entries <<< "${PATH:-}"
  for entry in "${entries[@]}"; do
    [[ "$entry" == "$directory" ]] && return 0
  done
  return 1
}

while (($#)); do
  case "$1" in
    --version)
      (($# >= 2)) || die '--version requires a value'
      version="$2"
      shift 2
      ;;
    --bin-dir)
      (($# >= 2)) || die '--bin-dir requires a value'
      bin_dir="$2"
      shift 2
      ;;
    --target)
      (($# >= 2)) || die '--target requires a value'
      target="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) die "unknown option: $1" ;;
  esac
done

main() {
  need curl
  need awk
  need sed
  need tar
  need mktemp
  need uname

  [[ -n "$bin_dir" ]] || die 'installation directory is empty; set --bin-dir or ENGR_INSTALL_DIR'
  version="$(if [[ -n "$version" ]]; then normalize_version "$version"; else latest_version; fi)"
  target="${target:-$(detect_target)}"

  local release_base="https://github.com/${REPOSITORY}/releases/download/v${version}"
  local manifest metadata artifact expected_sha sbom checksum_file archive actual_sha declared_sha expected_artifact extract destination temporary_destination reported
  temporary="$(mktemp -d "${TMPDIR:-/tmp}/engr-install.XXXXXX")"
  trap 'rm -rf "${temporary:-}"' EXIT

  manifest="${temporary}/release-manifest.json"
  download "${release_base}/release-manifest.json" "$manifest"
  [[ "$(manifest_value "$manifest" version)" == "$version" ]] \
    || die 'release manifest version does not match the requested version'
  [[ "$(manifest_protocol "$manifest")" == '1' ]] \
    || die 'release manifest protocol is not supported by this installer'

  metadata="$(manifest_target_metadata "$manifest" "$target")"
  [[ -n "$metadata" ]] || die "release v${version} has no artifact for target ${target}"
  IFS=$'\t' read -r artifact expected_sha sbom <<< "$metadata"
  [[ "$expected_sha" =~ ^[0-9a-f]{64}$ ]] || die 'release manifest contains an invalid SHA-256 value'
  expected_artifact="engr-${version}-${target}.tar.gz"
  [[ "$artifact" == "$expected_artifact" ]] || die "unexpected artifact name in release manifest: ${artifact}"

  checksum_file="${temporary}/${artifact}.sha256"
  archive="${temporary}/${artifact}"
  download "${release_base}/${artifact}.sha256" "$checksum_file"
  declared_sha="$(awk 'NR == 1 { print $1 }' "$checksum_file")"
  [[ "$declared_sha" == "$expected_sha" ]] || die 'checksum file and release manifest disagree'
  download "${release_base}/${artifact}" "$archive"
  actual_sha="$(sha256_file "$archive")"
  [[ "$actual_sha" == "$expected_sha" ]] || die 'downloaded artifact SHA-256 does not match the release manifest'
  [[ "$(tar -tzf "$archive")" == 'engr' ]] || die 'release archive must contain exactly one engr binary'

  extract="${temporary}/extract"
  mkdir -p "$extract" "$bin_dir"
  tar -xzf "$archive" -C "$extract"
  [[ -f "${extract}/engr" ]] || die 'release archive did not contain engr'
  destination="${bin_dir%/}/engr"
  temporary_destination="${bin_dir%/}/.engr.$$.$RANDOM.tmp"
  cp "${extract}/engr" "$temporary_destination"
  chmod 0755 "$temporary_destination"
  mv -f "$temporary_destination" "$destination"

  reported="$("$destination" version --json)" || die 'installed engr binary did not run successfully'
  [[ "$reported" == *"\"implementation_version\":\"${version}\""* ]] \
    || die 'installed engr binary reported a version different from the requested release'

  printf 'Installed Engr %s for %s at %s\n' "$version" "$target" "$destination"
  if ! path_contains "${bin_dir%/}"; then
    printf 'Add %s to PATH to invoke engr without its full path.\n' "${bin_dir%/}"
  fi
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
