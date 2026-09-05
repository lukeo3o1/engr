#!/bin/sh
# Collection: membership, order, priority, schedule, state — and the two
# uniqueness rules, each attacked rather than assumed.
set -u
cd /audit/r4/project
E=/audit/bin/engr-current
A=engr:obj:01m1sb6ksjfz0t81h8hmhk53bq   # the replacement decision object
B=engr:obj:01m1f5bshbeq1rpckc726yrk2b   # the evidence-rules object

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
