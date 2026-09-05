#!/bin/sh
# Work, end to end, on the Backlog topic this run actually raised — plus the
# interlock: a Backlog mutation that would remove the last point, and with it
# the subject, must refuse while that subject still has execution memory.
set -u
cd /audit/r4/project
E=/audit/bin/engr-current
W=engr:backlog:01m1sbcwt8f58s86bn768xjnbc
B=01a072b6
sec_token() { $E backlog show $B --format json | tr -d ' ' | grep -A3 "\"reference\":\"engr:backlog:[a-z0-9]*:$1\"" | grep '"expect"' | cut -d'"' -f4; }

echo "===== start, and the sidecar the write path produces ====="
$E work start "$W"; echo "exit=$?"
cat .engr/work/backlog/01a072b6-7348-7951-9419-753991d9556c.json; echo
echo
echo "===== items ====="
$E work item add "$W" --text "Re-observe the ls column against a divergent object"; echo "exit=$?"
$E work item add "$W" --text "Ask the reviewer which surface should move"; echo "exit=$?"
$E work item state "$W" --item 1 --state done; echo "exit=$?"
$E work item result "$W" --item 1 --text "ls says ok where verify exits 5; measured against ca6474a too"; echo "exit=$?"
$E work item commit "$W" --item 1 --commit "$(git rev-parse HEAD)"; echo "exit=$?"
echo
echo "===== dependencies and blockers ====="
$E work depend "$W" --on engr:obj:01m1sb6ksjfz0t81h8hmhk53bq --reason "The replacement object is where the answer lands."; echo "exit=$?"
$E work block "$W" --reason "Waiting on a ruling about which surface moves."; echo "exit=$?"
$E work summary "$W" --text "Measured at b02d05e and at ca6474a; the gap is pre-existing."; echo "exit=$?"
$E work show "$W"; echo "exit=$?"
echo
echo "===== the sidecar on disk, with every list populated ====="
cat .engr/work/backlog/01a072b6-7348-7951-9419-753991d9556c.json; echo
echo
echo "===== pause and resume ====="
$E work pause "$W"; echo "exit=$?"
$E work resume "$W"; echo "exit=$?"
echo
echo "===== the interlock: consume the last point while Work exists ====="
$E backlog consume $B --section 1 --expect "$(sec_token 1)"; echo "exit=$?"
echo
echo "===== drop the execution memory, then consume ====="
$E work unblock "$W" --position 1; echo "exit=$?"
$E work undepend "$W" --on engr:obj:01m1sb6ksjfz0t81h8hmhk53bq; echo "exit=$?"
$E work rm "$W"; echo "exit=$?"
$E backlog consume $B --section 1 --expect "$(sec_token 1)"; echo "exit=$?"
$E backlog ls; echo "exit=$?"
echo "backlog files left: $(ls .engr/backlog 2>/dev/null | wc -l); work files left: $(find .engr/work -type f 2>/dev/null | wc -l)"
