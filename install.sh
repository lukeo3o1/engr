#!/usr/bin/env bash
# Install engr.
#
# By default this downloads the archive for your platform from the `latest`
# release, checks it against the published SHA256SUMS, and installs it — no
# checkout and no Rust toolchain needed. `--from-source` builds from a checkout
# instead, which is the path that works with no network and nothing to trust.
#
# There are no version numbers. `latest` moves, and the binary reports the commit
# it was built from; that is what the version line at the end names.
#
# It never modifies PATH; it says what to add and leaves that to you.
#
# Exit codes: 2 usage, 3 this environment cannot do it (no download tool, no
# toolchain, no published archive for this platform), 8 the download, build or
# verification failed.

set -eu

RELEASE="https://github.com/lukeo3o1/engr/releases/download/latest"
BIN_DIR="${HOME}/.local/bin"
PROFILE="release"
FROM_SOURCE=""

usage() {
	cat <<'EOF'
Usage: install.sh [--bin-dir DIR] [--from-source [--debug]]

  --bin-dir DIR   where to install (default: ~/.local/bin)
  --from-source   build from an engr checkout instead of downloading
  --debug         with --from-source, build the debug profile
  -h, --help      this message
EOF
}

while [ "$#" -gt 0 ]; do
	case "$1" in
	--bin-dir)
		[ "$#" -ge 2 ] || {
			echo "install.sh: --bin-dir needs a directory" >&2
			exit 2
		}
		BIN_DIR="$2"
		shift 2
		;;
	--from-source)
		FROM_SOURCE="yes"
		shift
		;;
	--debug)
		PROFILE="debug"
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		echo "install.sh: unknown argument $1" >&2
		usage >&2
		exit 2
		;;
	esac
done

if [ -z "${FROM_SOURCE}" ] && [ "${PROFILE}" = "debug" ]; then
	echo "install.sh: --debug only applies to --from-source" >&2
	exit 2
fi

fetch() {
	if command -v curl >/dev/null 2>&1; then
		curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"
	elif command -v wget >/dev/null 2>&1; then
		wget -q -O "$2" "$1"
	else
		echo "install.sh: neither curl nor wget is available" >&2
		exit 3
	fi
}

sha256() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | cut -d' ' -f1
	elif command -v shasum >/dev/null 2>&1; then
		shasum -a 256 "$1" | cut -d' ' -f1
	else
		echo "install.sh: no sha256 tool (sha256sum or shasum)" >&2
		exit 3
	fi
}

# Only the three platforms the release workflow builds. Anywhere else the honest
# answer is to say so and point at --from-source, rather than downloading an
# archive that cannot run here.
triple() {
	case "$(uname -s):$(uname -m)" in
	Linux:x86_64) echo "x86_64-unknown-linux-gnu" ;;
	Darwin:arm64 | Darwin:aarch64) echo "aarch64-apple-darwin" ;;
	*) return 1 ;;
	esac
}

if [ -n "${FROM_SOURCE}" ]; then
	REPO="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)"
	[ -f "${REPO}/Cargo.toml" ] || {
		echo "install.sh: --from-source needs an engr checkout (no Cargo.toml beside this script)" >&2
		exit 2
	}
	command -v cargo >/dev/null 2>&1 || {
		echo "install.sh: cargo not found — install Rust from https://rustup.rs" >&2
		exit 3
	}

	echo "building    ${PROFILE}"
	# --target-dir on the command line beats CARGO_TARGET_DIR and any config, so
	# the path read below is certain to be the path just written. Guessing
	# `target/` instead is worse than a miss: a stale binary left there from an
	# earlier build gets installed and then "verified", which is the one thing
	# this must not do.
	TARGET_DIR="${REPO}/target"
	if [ "${PROFILE}" = "release" ]; then
		(cd "${REPO}" && cargo build --release --quiet -p engr --target-dir "${TARGET_DIR}")
	else
		(cd "${REPO}" && cargo build --quiet -p engr --target-dir "${TARGET_DIR}")
	fi

	BUILT="${TARGET_DIR}/${PROFILE}/engr"
	[ -x "${BUILT}" ] || {
		echo "install.sh: expected a binary at ${BUILT}" >&2
		exit 8
	}
else
	TRIPLE="$(triple)" || {
		echo "install.sh: no archive is published for $(uname -s)/$(uname -m); use --from-source" >&2
		exit 3
	}
	ARCHIVE="engr-${TRIPLE}.tar.gz"
	WORK="$(mktemp -d)"
	trap 'rm -rf "${WORK}"' EXIT

	echo "downloading ${ARCHIVE}"
	fetch "${RELEASE}/${ARCHIVE}" "${WORK}/${ARCHIVE}" || {
		echo "install.sh: could not download ${ARCHIVE}" >&2
		echo "            if no \`latest\` release has been published yet, use --from-source" >&2
		exit 8
	}
	fetch "${RELEASE}/SHA256SUMS" "${WORK}/SHA256SUMS" || {
		echo "install.sh: could not download SHA256SUMS" >&2
		exit 8
	}

	# The sums come from the same release as the archive, so this catches a
	# truncated or corrupted download — not a compromised release. Worth doing
	# for the first; do not read it as the second.
	EXPECTED="$(grep " ${ARCHIVE}\$" "${WORK}/SHA256SUMS" | cut -d' ' -f1)"
	[ -n "${EXPECTED}" ] || {
		echo "install.sh: SHA256SUMS does not list ${ARCHIVE}" >&2
		exit 8
	}
	ACTUAL="$(sha256 "${WORK}/${ARCHIVE}")"
	[ "${ACTUAL}" = "${EXPECTED}" ] || {
		echo "install.sh: checksum mismatch for ${ARCHIVE}" >&2
		echo "            expected ${EXPECTED}" >&2
		echo "            got      ${ACTUAL}" >&2
		exit 8
	}
	echo "checksum    ok"

	tar -xzf "${WORK}/${ARCHIVE}" -C "${WORK}"
	BUILT="${WORK}/engr"
	[ -f "${BUILT}" ] || {
		echo "install.sh: ${ARCHIVE} does not contain engr" >&2
		exit 8
	}
fi

mkdir -p "${BIN_DIR}"
# Install to a temporary name and move it into place, so a running engr is never
# overwritten underneath itself.
STAGED="${BIN_DIR}/.engr.$$"
cp "${BUILT}" "${STAGED}"
chmod 755 "${STAGED}"
mv -f "${STAGED}" "${BIN_DIR}/engr"
echo "installed   ${BIN_DIR}/engr"

# Prove the thing that was installed actually runs, rather than trusting the copy.
VERSION="$("${BIN_DIR}/engr" --version)" || {
	echo "install.sh: the installed binary did not run" >&2
	exit 8
}
echo "verified    ${VERSION}"

case ":${PATH}:" in
*":${BIN_DIR}:"*) ;;
*)
	echo
	echo "${BIN_DIR} is not on your PATH. Add it:"
	echo "  export PATH=\"${BIN_DIR}:\$PATH\""
	;;
esac
