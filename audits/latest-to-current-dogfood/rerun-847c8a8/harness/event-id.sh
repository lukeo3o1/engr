#!/bin/sh
# Round 25/4: an Event whose id is the uppercase spelling of a canonical UUID,
# resealed over that spelling so its own digest verifies. Canonicality is a
# separate question from parsing, and the check must ask it.
# Host-side: node is not in the image.
set -u
cd "$(dirname "$0")/.."
rm -rf ev && cp -a checkpoints/migrated ev
ID=01a05e55-ee46-7a91-983f-fa79b297b10c
S=ev/.engr/eventstore/objects/$ID.jsonl
before=$(sha256sum "$S" | cut -d' ' -f1)
node harness/evseal.js "$S" "$ID"
after=$(sha256sum "$S" | cut -d' ' -f1)
[ "$before" = "$after" ] && echo "NO-OP! the stream did not move" || echo "stream moved: $before -> $after"
echo "--- and its own seal verifies, checked independently ---"
node harness/evseal.js "$S" "$ID" check
