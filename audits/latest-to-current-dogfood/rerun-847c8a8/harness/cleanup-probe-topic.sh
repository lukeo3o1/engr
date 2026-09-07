#!/bin/sh
# Consume the topic work-rm-message.sh raised, through the tool rather than by
# deleting the file, so the workspace the rest of the run sees is one engr made.
set -u
cd /audit/r8/project
E=/audit/bin/engr-current
B=$($E backlog ls | sed -n 's/^\([0-9a-f]\{8\}\) .*Work rm message probe.*/\1/p' | head -1)
[ -n "$B" ] || { echo "nothing to clean up"; exit 0; }
tok=$($E backlog show "$B" --format json | tr -d ' ' | grep -A3 '"reference":"engr:backlog:[a-z0-9]*:1"' | grep '"expect"' | cut -d'"' -f4)
$E backlog consume "$B" --section 1 --expect "$tok"; echo "exit=$?"
$E backlog ls; echo "exit=$?"
echo "backlog files left: $(ls .engr/backlog 2>/dev/null | wc -l); work files left: $(find .engr/work -type f 2>/dev/null | wc -l)"
