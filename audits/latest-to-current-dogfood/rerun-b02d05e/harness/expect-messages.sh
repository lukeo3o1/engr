#!/bin/sh
# `expect` is two levels: an object of topic-level tokens (rename, add) plus one
# token per point. Every operation that needs one is asked what it tells a
# reader to pass, and then the reader's literal reading of that advice is tried.
set -u
cd /audit/r4/project
E=/audit/bin/engr-current
B=01a072c3
topic_add=$($E backlog show $B --format json | sed -n '/"expect": {/,/^  }/p' | grep '"add"' | cut -d'"' -f4)
topic_rename=$($E backlog show $B --format json | sed -n '/"expect": {/,/^  }/p' | grep '"rename"' | cut -d'"' -f4)
point1=$($E backlog show $B --format json | tr -d ' ' | grep -A3 '"reference":"engr:backlog:[a-z0-9]*:1"' | grep '"expect"' | cut -d'"' -f4)
echo "topic-level add   : $topic_add"
echo "topic-level rename: $topic_rename"
echo "point 1           : $point1"
echo

say() { printf '%-46s %s\n' "$1" "$2"; }
run() { out=$("$@" 2>&1); printf '  exit=%-3s %s\n' "$?" "$(printf '%s' "$out" | tr '\n' '|' | cut -c1-160)"; }

echo "===== what each operation says when the token is missing ====="
say "backlog rename (topic-level)" ""; run $E backlog rename $B --title "x"
say "backlog add (topic-level)" "";    run $E backlog add $B --text "x"
say "backlog revise (point-level)" ""; run $E backlog revise $B --section 1 --text "x"
say "backlog subjects (point-level)" "";run $E backlog subjects $B --section 1 --subject engr:obj:01m1f5ax7bedrbew8012yq3ma9
say "backlog produced (point-level)" "";run $E backlog produced $B --section 1 --target engr:obj:01m1f5ax7bedrbew8012yq3ma9
say "backlog consume (point-level)" ""; run $E backlog consume $B --section 2
echo
echo "===== and what happens if a reader follows that advice literally ====="
say "rename, given the point's token" ""; run $E backlog rename $B --title "x" --expect "$point1"
say "add, given the point's token" "";    run $E backlog add $B --text "x" --expect "$point1"
say "revise, given the topic's add token" ""; run $E backlog revise $B --section 1 --text "x" --expect "$topic_add"
say "rename, given the topic's rename token" ""; run $E backlog rename $B --title "renamed properly" --expect "$topic_rename"
