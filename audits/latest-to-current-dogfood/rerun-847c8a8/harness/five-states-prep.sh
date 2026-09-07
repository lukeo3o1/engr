#!/bin/sh
# Build one workspace per Object integrity state, so the five answers the
# classifier now has can be asked of every surface at one instant.
#
# Round 32 moved `show --format json`'s `integrity` member off its own private
# match and onto `view::object_fault`, the classifier the listing uses. That is
# a new consumer for four pre-existing answers as well as a new answer, so each
# of the five is built here and asked of all four surfaces at both heads.
#
# Host-side: node is not in the container image.
set -u
cd "$(dirname "$0")/.."
ID=01a05e55-74eb-7370-b771-0008bd71d149

# ok -- nothing wrong with it
rm -rf st-ok && cp -a checkpoints/migrated st-ok

# projection_missing -- the file is gone, the stream is intact
rm -rf st-missing && cp -a checkpoints/migrated st-missing
rm st-missing/.engr/objects/$ID.json
[ -e st-missing/.engr/objects/$ID.json ] && echo "NO-OP! st-missing" || echo "st-missing: projection removed"

# tampered -- bytes edited and NOT resealed, so the Object's own seal fails
rm -rf st-tampered && cp -a checkpoints/migrated st-tampered
T=st-tampered/.engr/objects/$ID.json
before=$(sha256sum "$T" | cut -d' ' -f1)
sed -i 's/"title":"Migration continuity audit"/"title":"Migration continuity audiT"/' "$T"
after=$(sha256sum "$T" | cut -d' ' -f1)
[ "$before" = "$after" ] && echo "NO-OP! st-tampered" || echo "st-tampered: $before -> $after"

# divergent -- resealed, so the seal passes and no admitted Event produced it
rm -rf st-divergent && cp -a checkpoints/migrated st-divergent
D=st-divergent/.engr/objects/$ID.json
before=$(sha256sum "$D" | cut -d' ' -f1)
node harness/forge.js set "$D" 1 header '"A heading nobody ever admitted"' >/dev/null
after=$(sha256sum "$D" | cut -d' ' -f1)
[ "$before" = "$after" ] && echo "NO-OP! st-divergent" || echo "st-divergent: $before -> $after"
echo "st-divergent seals, checked independently:"
node harness/jcs.js "$D" | sed -n '1,5p'

# unreplayable -- a correctly sealed, contiguous Event that cannot be applied
rm -rf st-unreplayable && cp -a checkpoints/migrated st-unreplayable
S=st-unreplayable/.engr/eventstore/objects/$ID.jsonl
before=$(sha256sum "$S" | cut -d' ' -f1)
node harness/appendev.js "$S" "$ID" 2 section.deleted.v1 '{"section":99}' >/dev/null
after=$(sha256sum "$S" | cut -d' ' -f1)
[ "$before" = "$after" ] && echo "NO-OP! st-unreplayable" || echo "st-unreplayable: $before -> $after"
node harness/evseal.js "$S" "$ID" check | tail -2
