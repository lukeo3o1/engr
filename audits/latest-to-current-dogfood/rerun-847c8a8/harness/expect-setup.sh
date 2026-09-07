#!/bin/sh
# A fresh two-point topic for the `expect` message probe. The one the domain
# exercise made was consumed by the Work interlock step, and a probe that runs
# against a topic that no longer exists refuses for the wrong reason.
set -u
cd /audit/r8/project
E=/audit/bin/engr-current
$E backlog new --title "Expect-token wording" --text "The first point, for the token probe." | head -2
id=$($E backlog ls | sed -n 's/^\([0-9a-f]\{8\}\) .*Expect-token wording.*/\1/p' | head -1)
[ -n "$id" ] || { echo "NO BACKLOG ID"; exit 1; }
add=$($E backlog show "$id" --format json | sed -n '/"expect": {/,/^  }/p' | grep '"add"' | cut -d'"' -f4)
$E backlog add "$id" --text "A second point, so consume has something to name." --expect "$add" | head -1
echo "$id" > /audit/r8/evidence/expect-backlog-id.txt
echo "backlog: $id"
