#!/bin/sh
# Round 29's three states, built to differ only in which question they answer:
#   dep-e  a Section removed and the Object resealed   -> divergence
#   dep-f  the same removal, unsealed                  -> ordinary damage
#   dep-g  removed through an admitted deletion Event  -> genuinely absent
# Host-side: node is not in the image.
set -u
cd "$(dirname "$0")/.."
T=.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
for d in dep-e dep-f dep-g; do rm -rf "$d"; cp -a checkpoints/migrated "$d"; done
for pair in "dep-e del" "dep-f rawdel"; do
  d=${pair% *}; op=${pair#* }
  before=$(sha256sum "$d/$T" | cut -d' ' -f1)
  node harness/forge.js "$op" "$d/$T" 1
  after=$(sha256sum "$d/$T" | cut -d' ' -f1)
  [ "$before" = "$after" ] && echo "NO-OP! $d unchanged" || echo "$d: $before -> $after"
done
