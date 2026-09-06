#!/bin/sh
# A correctly sealed, correctly framed, revision-contiguous Event that cannot be
# applied. It is the one broken history that gets past the framing rules, and
# the state r4's F-3 was about: repair used to call the Object sound here.
# Host-side: node is not in the image.
set -u
cd "$(dirname "$0")/.."
rm -rf tail && cp -a checkpoints/migrated tail
ID=01a05e55-74eb-7370-b771-0008bd71d149
S=tail/.engr/eventstore/objects/$ID.jsonl
before=$(sha256sum "$S" | cut -d' ' -f1)
node harness/appendev.js "$S" "$ID" 2 section.deleted.v1 '{"section":99}'
after=$(sha256sum "$S" | cut -d' ' -f1)
[ "$before" = "$after" ] && echo "NO-OP! the stream did not move" || echo "stream: $before -> $after"
echo "--- the appended event, and its seal checked independently ---"
tail -1 "$S"
node harness/evseal.js "$S" "$ID" check
