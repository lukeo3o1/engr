#!/bin/sh
# Claude Code Stop hook: refuse the first attempt to finish while the repository
# has moved since execution memory was last written.
#
# The instant a step works is the instant its item goes unticked, and the end of
# a session is the worst moment to catch it — so this asks once, and only when
# there is evidence: a commit or a modified file newer than the newest sidecar.
# Asked once means `stop_hook_active` lets the second attempt through; a check
# that can trap an agent in a loop teaches it to satisfy the check rather than
# the handoff.
input=$(cat)
case "$input" in
  *'"stop_hook_active":true'* | *'"stop_hook_active": true'*) exit 0 ;;
esac
cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
[ -d .engr ] || exit 0
command -v engr >/dev/null 2>&1 || exit 0

latest=$(engr work ls 2>/dev/null |
  grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]+Z' | sort | tail -n 1)
if [ -n "$latest" ]; then
  commits=$(git log --since="$latest" --format='%h %s' -- . ':!.engr' 2>/dev/null)
  modified=$(git status --porcelain -- . ':!.engr' 2>/dev/null | cut -c4- |
    while IFS= read -r path; do
      [ -e "$path" ] &&
        [ -n "$(find "$path" -maxdepth 0 -newermt "$latest" 2>/dev/null)" ] &&
        echo "$path (modified)"
    done)
  moved="$commits
$modified"
  since="since the newest sidecar was written ($latest)"
else
  # No sidecar at all: only uncommitted work is evidence, because a commit
  # from before this session says nothing about what this one did.
  moved=$(git status --porcelain -- . ':!.engr' 2>/dev/null)
  since="and no execution memory exists"
fi
moved=$(printf '%s\n' "$moved" | sed '/^$/d' | head -n 10)
[ -n "$moved" ] || exit 0

cat >&2 <<MESSAGE
engr: the repository moved $since:
$moved

Before stopping, make what the next session reads true:
- close finished items with their evidence: item state done, item result, item commit
- record a decision that settled, and stage a question you are leaving, in the backlog
- rewrite the summary to where things stand and which item is next
If none of this needs a handoff, stop again; this asks only once.
MESSAGE
exit 2
