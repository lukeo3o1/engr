#!/bin/sh
# Three kinds of damage the cheap listing now meets alone, built from the same
# migrated checkpoint so only the damage differs.
#
#   dep-b   projection removed, its Event stream left intact
#   dep-c   projection present and unreadable
#   dep-d   projection edited and NOT resealed  (ordinary tampering)
set -u
cd "$(dirname "$0")/.."
T=.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
for d in dep-b dep-c dep-d; do rm -rf "$d"; cp -a checkpoints/migrated "$d"; done

echo "--- dep-b: the projection is gone, the stream is not ---"
before=$(sha256sum "dep-b/$T" | cut -d' ' -f1)
rm "dep-b/$T"
[ -e "dep-b/$T" ] && echo "NO-OP! still there" || echo "removed (was $before)"
ls dep-b/.engr/eventstore/objects/ | wc -l | sed 's/^/streams still present: /'

echo "--- dep-c: the projection is unreadable ---"
before=$(sha256sum "dep-c/$T" | cut -d' ' -f1)
printf 'this is not an object\n' > "dep-c/$T"
after=$(sha256sum "dep-c/$T" | cut -d' ' -f1)
[ "$before" = "$after" ] && echo "NO-OP!" || echo "overwritten: $before -> $after"

echo "--- dep-d: one Section's wording edited, nothing resealed ---"
before=$(sha256sum "dep-d/$T" | cut -d' ' -f1)
node -e '
const fs=require("fs"); const f=process.argv[1];
const o=JSON.parse(fs.readFileSync(f,"utf8"));
o.sections[0].text = "Wording nobody ever admitted, and nothing was resealed.";
fs.writeFileSync(f, JSON.stringify(o));
' "dep-d/$T"
after=$(sha256sum "dep-d/$T" | cut -d' ' -f1)
[ "$before" = "$after" ] && echo "NO-OP!" || echo "edited: $before -> $after"
node harness/jcs.js "dep-d/$T" || echo "(both seals must fail, which is the point)"
