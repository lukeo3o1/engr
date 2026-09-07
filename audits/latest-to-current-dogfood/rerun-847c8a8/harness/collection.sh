#!/bin/sh
# Collection: membership, order, priority, schedule, state — and the two
# uniqueness rules, each attacked rather than assumed.
set -u
cd /audit/r8/project
E=/audit/bin/engr-current

# Both targets are resolved from the running workspace, never hardcoded. r7
# carried a previous run's compact id here and the membership-uniqueness probes
# refused with `does not exist` -- a refusal for the wrong reason, which hides
# whatever the probe was for. The replacement object is created fresh by
# supersession every run, so its id moves; the evidence-rules object is one of
# the three migrated fixture objects, so its id does not. Both are looked up the
# same way regardless, and the script refuses rather than measuring nothing.
ref_of() { # $1 = anything `show` accepts
  $E show "$1" --format json 2>/dev/null \
    | sed -n 's/.*"reference": *"\(engr:obj:[a-z0-9]*\)".*/\1/p' | head -1
}
NEWID=$($E ls --all 2>/dev/null | sed -n 's/^\([0-9a-f-]*\) .*Continuity carried forward.*/\1/p' | head -1)
A=$(ref_of "$NEWID")
B=$(ref_of 01a05e55-e6)
RECORDED=$(tr -d '\357\273\277\r' < /audit/r8/evidence/replacement-object.txt 2>/dev/null | head -1)
echo "resolved A = $A   (this run's replacement object, $NEWID; supersession recorded $RECORDED)"
echo "resolved B = $B   (the evidence-rules object, 01a05e55-e6)"
[ -n "$A" ] || { echo "REFUSING: this run's replacement object did not resolve -- run supersession first"; exit 1; }
[ -n "$B" ] || { echo "REFUSING: the evidence-rules object did not resolve"; exit 1; }
[ "$NEWID" = "$RECORDED" ] || echo "WARNING: ls says $NEWID, supersession recorded $RECORDED"

echo "===== new ====="
$E collection new release-2026 --title "Baseline v1 release" --description "What has to be true before the merge."; echo "exit=$?"
echo
echo "===== members, order and priority ====="
$E collection add release-2026 --target $A --order 10 --priority high --reason "Gates the merge."; echo "exit=$?"
$E collection add release-2026 --target $B --order 20; echo "exit=$?"
echo "-- the same target again, with different metadata --"
$E collection add release-2026 --target $A --order 30 --priority low; echo "exit=$?"
echo "-- a second member at a rank that is taken --"
$E collection order release-2026 --target $B --order 10; echo "exit=$?"
echo
echo "===== schedule ====="
$E collection schedule release-2026 --start 2026-09-01 --target-date 2026-09-20 --end 2026-09-30; echo "exit=$?"
echo "-- an end before its start --"
$E collection schedule release-2026 --start 2026-09-30 --end 2026-09-01; echo "exit=$?"
echo
echo "===== state, and the file on disk ====="
$E collection state release-2026 --state completed; echo "exit=$?"
$E collection show release-2026; echo "exit=$?"
cat .engr/collections/release-2026.json; echo
echo
echo "===== an id the grammar refuses ====="
$E collection new "Release 2026" --title "spaces and capitals"; echo "exit=$?"
$E collection new "$(printf 'x%.0s' $(seq 1 40))" --title "too long"; echo "exit=$?"
echo
echo "===== unrank, clear priority, remove ====="
$E collection order release-2026 --target $B; echo "exit=$?"
$E collection priority release-2026 --target $A; echo "exit=$?"
$E collection rm release-2026 --target $B; echo "exit=$?"
$E collection show release-2026; echo "exit=$?"
cat .engr/collections/release-2026.json; echo
