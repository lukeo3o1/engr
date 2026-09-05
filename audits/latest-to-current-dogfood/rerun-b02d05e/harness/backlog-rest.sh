#!/bin/sh
# The rest of the Backlog surface, each operation reading its own fresh token.
# `expect` is an object of per-operation tokens plus one per point, so every step
# re-reads rather than reusing what the last step saw.
set -u
cd /audit/r4/project
E=/audit/bin/engr-current
B=01a072b6
j() { $E backlog show $B --format json; }
op_token()  { j | sed -n '/"expect": {/,/^  }/p' | grep "\"$1\"" | cut -d'"' -f4; }
sec_token() { j | sed -n "/\"id\": $1,/!b" >/dev/null; j | tr -d ' ' | grep -A3 "\"reference\":\"engr:backlog:[a-z0-9]*:$1\"" | grep '"expect"' | cut -d'"' -f4; }

echo "===== subjects: replace what a point concerns ====="
$E backlog subjects $B --section 1 --subject engr:obj:01m1sb6ksjfz0t81h8hmhk53bq --expect "$(sec_token 1)"; echo "exit=$?"
echo
echo "===== produced: record durable knowledge, without resolving the point ====="
$E backlog produced $B --section 2 --target engr:obj:01m1sb6ksjfz0t81h8hmhk53bq --expect "$(sec_token 2)"; echo "exit=$?"
$E backlog show $B; echo "exit=$?"
echo
echo "===== merge: two points become one ====="
t1=$(sec_token 1); t2=$(sec_token 2)
$E backlog merge $B --into 1 --section 2 --text "Whether ls and repair should say what verify says about the same object." --expect "$t1" --expect "$t2"; echo "exit=$?"
echo
echo "===== the merged point, and the produced outcome it inherited ====="
j
echo
echo "===== review exhaustion: attempt 5 against audit-scope's ceiling of 3 ====="
$E backlog revise $B --section 1 --text "Whether ls and repair should say what verify says about the same object; raised again on attempt five." --expect "$(sec_token 1)" --attempt 5; echo "exit=$?"
$E backlog show $B; echo "exit=$?"
echo
echo "===== and it is persisted, not only printed ====="
j
