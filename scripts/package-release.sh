#!/usr/bin/env bash
set -euo pipefail

target="$1"
archive_kind="$2"
ref_name="${GITHUB_REF_NAME:-}"
package_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
if [[ "$ref_name" == v* ]]; then
  version="${ref_name#v}"
else
  version="$package_version"
fi
if [[ -z "$version" || -z "$package_version" ]]; then
  echo "could not determine release version" >&2
  exit 2
fi
if [[ "$version" != "$package_version" ]]; then
  echo "tag version $version does not match Cargo package version $package_version" >&2
  exit 2
fi
binary="target/${target}/release/engr"
if [[ "$target" == *windows* ]]; then
  binary="${binary}.exe"
fi

mkdir -p release/stage
cp "$binary" "release/stage/$(basename "$binary")"
archive="engr-${version}-${target}"
if [[ "$archive_kind" == zip ]]; then
  (cd release/stage && 7z a "../${archive}.zip" "$(basename "$binary")")
  artifact="release/${archive}.zip"
else
  tar -C release/stage -czf "release/${archive}.tar.gz" "$(basename "$binary")"
  artifact="release/${archive}.tar.gz"
fi
if command -v sha256sum >/dev/null 2>&1; then
  checksum="$(sha256sum "$artifact" | awk '{print $1}')"
else
  checksum="$(shasum -a 256 "$artifact" | awk '{print $1}')"
fi
printf '%s  %s\n' "$checksum" "$(basename "$artifact")" > "${artifact}.sha256"

sbom_name="sbom-${target}.cdx.json"
sbom_prefix="${sbom_name%.cdx.json}"
find crates/engr -maxdepth 1 -type f -name "${sbom_prefix}*" -delete
cargo cyclonedx \
  --manifest-path crates/engr/Cargo.toml \
  --format json \
  --target "$target" \
  --override-filename "$sbom_prefix"
generated_sboms=()
while IFS= read -r generated_sbom; do
  generated_sboms+=("$generated_sbom")
done < <(find crates/engr -maxdepth 1 -type f -name "${sbom_prefix}*" -print)
if (( ${#generated_sboms[@]} != 1 )); then
  echo "CycloneDX did not produce exactly one SBOM for $target" >&2
  exit 2
fi
mv "${generated_sboms[0]}" "release/$sbom_name"

cat > "release/manifest-target-${target}.json" <<EOF
{
  "target": "${target}",
  "path": "$(basename "$artifact")",
  "sha256": "${checksum}",
  "sbom": "$sbom_name"
}
EOF
rm -rf release/stage
