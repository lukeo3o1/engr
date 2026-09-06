#!/bin/sh
# A target Section that seals perfectly and is not what its history produced.
# The forged field is one the Ref does NOT select (header, not text), so
# neither drift nor staleness can see it: only the target's own history can.
# Host-side, because node is not in the image.
set -u
cd "$(dirname "$0")/.."
rm -rf dep-a && cp -a checkpoints/migrated dep-a
T=dep-a/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
before=$(sha256sum "$T" | cut -d' ' -f1)
node harness/forge.js set "$T" 1 header '"A heading nobody ever admitted"'
after=$(sha256sum "$T" | cut -d' ' -f1)
echo "before: $before"
echo "after : $after"
[ "$before" = "$after" ] && echo "NO-OP! the probe changed nothing" || echo "the target moved, and every seal on it still verifies"
node harness/jcs.js "$T" | sed -n '1,4p'
