#!/bin/sh
# The premise every forgery below rests on: an implementation of the digest
# contracts that has never seen engr's own code reproduces every stored seal.
# RFC 8785 JCS + SHA-256 from scratch for Sections and Objects,
# EventDigestContract 1 for the streams, and RefDigest assembled by hand from
# #66 6.5 with the historical values taken from git at the pinned commit.
#
# Host-side: node is not in the container image.
set -u
cd "$(dirname "$0")/.."
P=project/.engr
for f in "$P"/objects/*.json; do
  id=$(basename "$f" .json)
  echo "=== object $id ==="
  node harness/jcs.js "$f" || echo "PREMISE BROKEN"
done
for f in "$P"/eventstore/objects/*.jsonl; do
  id=$(basename "$f" .jsonl)
  echo "=== stream $id ==="
  node harness/evseal.js "$f" "$id" check
done
echo "=== migrated RefDigest, assembled by hand ==="
mkdir -p scratch
git -C project show 221a5db38924d39449ed1c76a3beddf11e302113:.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json > scratch/historical-74.json
node harness/refdigest.js \
  "$P/objects/01a05e55-e62b-75c3-8b32-6c388dec4c4b.json" \
  scratch/historical-74.json 2 1 || echo "PREMISE BROKEN"
