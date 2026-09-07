#!/bin/sh
# Round 31's first P2, and the control that keeps it honest. One workspace, two
# Objects that differ only in what is wrong with them:
#
#   -74   the projection removed, its Event stream intact  -> missing storage
#   -ee   the projection one revision behind its history   -> a healthy tail
#
# The second is what stops "reported" from meaning "reported as anything".
set -u
cd /audit/r7
E=/audit/bin/engr-current
rm -rf as && cp -a checkpoints/migrated as
T=as/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
B=as/.engr/objects/01a05e55-ee46-7a91-983f-fa79b297b10c.json

echo "--- the healthy tail: admit a section, then put the projection back ---"
cp "$B" as/behind.json
out=$($E --root as prepare --object 01a05e55-ee --add --no-based-on --text "Admitted, and the projection never caught up." 2>&1)
code=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p' | head -1)
$E --root as confirm "CONFIRM $code" | head -1
before=$(sha256sum "$B" | cut -d' ' -f1)
cp as/behind.json "$B"; rm as/behind.json
after=$(sha256sum "$B" | cut -d' ' -f1)
[ "$before" = "$after" ] && echo "NO-OP! the rewind changed nothing" || echo "rewound: $before -> $after"
echo "stored sections: $(grep -o '"id":[0-9]*' "$B" | wc -l), stream events: $(wc -l < as/.engr/eventstore/objects/01a05e55-ee46-7a91-983f-fa79b297b10c.jsonl)"

echo "--- the missing projection: the file goes, the stream stays ---"
gone=$(sha256sum "$T" | cut -d' ' -f1)
rm "$T"
[ -e "$T" ] && echo "NO-OP! still there" || echo "removed (was $gone)"
echo "streams still present: $(ls as/.engr/eventstore/objects | wc -l)"
