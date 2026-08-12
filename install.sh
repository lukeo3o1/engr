#!/usr/bin/env bash
# Build engr from this checkout and install it.
#
# v0 is not released, so there is no verified archive to download. This builds
# from the source you already have, which is the only honest option — an
# installer pointing at a release tag that does not exist would fail in a way
# that looks like a network problem.
#
# It never modifies PATH; it says what to add and leaves that to you.

set -eu

BIN_DIR="${HOME}/.local/bin"
PROFILE="release"

usage() {
	cat <<'EOF'
Usage: install.sh [--bin-dir DIR] [--debug]

  --bin-dir DIR   where to install (default: ~/.local/bin)
  --debug         build the debug profile instead of release
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

REPO="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)"
[ -f "${REPO}/Cargo.toml" ] || {
	echo "install.sh: run this from an engr checkout (no Cargo.toml beside it)" >&2
	exit 2
}

command -v cargo >/dev/null 2>&1 || {
	echo "install.sh: cargo not found — install Rust from https://rustup.rs" >&2
	exit 3
}

echo "building    ${PROFILE}"
if [ "${PROFILE}" = "release" ]; then
	(cd "${REPO}" && cargo build --release --quiet -p engr)
else
	(cd "${REPO}" && cargo build --quiet -p engr)
fi

BUILT="${REPO}/target/${PROFILE}/engr"
[ -x "${BUILT}" ] || {
	echo "install.sh: expected a binary at ${BUILT}" >&2
	exit 8
}

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
