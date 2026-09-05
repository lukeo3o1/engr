#!/bin/sh
# Round 25's finding 5, re-observed: `verify` walked the *stored* projection for
# dependencies, so a Ref admitted in a crash tail was never checked. The probe
# must have no Ref in the stored bytes at all — the dependent's projection is
# rolled back to before the admission while the Event stays.
set -u
cd /audit/r4
E=/audit/bin/engr-current
AU=01a05e55-74eb-7370-b771-0008bd71d149
EE=01a05e55-ee46-7a91-983f-fa79b297b10c
rm -rf ct && cp -a crash-runs/baseline ct
cp "ct/.engr/objects/$EE.json" /tmp/rev1.json

out=$($E --root ct prepare --object 01a05e55-ee --add --no-based-on --text "Wording that depends on the audit object, admitted just before the crash." --ref 01a05e55-74:1 text 2>&1)
c=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p' | head -1)
echo "prepared: $c"
$E --root ct confirm "CONFIRM $c" | head -1

echo "-- the crash: the projection goes back to rev 1, the Event stays --"
cp /tmp/rev1.json "ct/.engr/objects/$EE.json"
echo "stored refs in the projection: $(grep -c '\"refs\"' "ct/.engr/objects/$EE.json")"
echo "events in the stream         : $(wc -l < "ct/.engr/eventstore/objects/$EE.jsonl")"

echo "-- and the target is made unreadable --"
sed -i 's/"state":"open"/"state":"not-a-state"/' "ct/.engr/objects/$AU.json"

echo "-- verify must still check the dependency the tail admitted --"
$E --root ct verify 01a05e55-ee 2>&1; echo "exit=$?"
echo
echo "-- and show, at the same instant --"
$E --root ct show 01a05e55-ee 2>&1 | tail -6; echo "exit=$?"
