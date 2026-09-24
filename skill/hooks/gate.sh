#!/bin/sh
# Claude Code PreToolUse hook (Edit|Write): hold the next edit when the handoff
# has fallen too far behind the work.
#
# The heartbeat's reminder was delivered and ignored: an agent in the middle of
# a change reads a line of hook context as noise and carries on. A refused tool
# call cannot be read past — the agent has to answer it before it can edit
# again. So this refuses an edit once HEARTBEAT_GATE tool calls have gone by
# without a write to engr, and only when there is evidence of unrecorded work:
# a file outside .engr modified after the newest sidecar, a commit after it, or
# a dirty tree with no sidecar at all. Any write to engr resets the count
# (heartbeat.sh keeps it), so the refusal lasts exactly until the handoff is
# brought up to date — and says that a one-line summary of where things stand
# is a real answer when nothing else changed.
input=$(cat)
cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
[ -d .engr ] || exit 0
command -v engr >/dev/null 2>&1 || exit 0

limit=${HEARTBEAT_GATE:-20}
session=$(printf '%s' "$input" | sed -n 's/.*"session_id": *"\([^"]*\)".*/\1/p' | head -n 1)
count=$(cat "${TMPDIR:-/tmp}/engr-heartbeat-${session:-unknown}" 2>/dev/null || echo 0)
[ "$count" -ge "$limit" ] || exit 0

latest=$(engr work ls 2>/dev/null |
  grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]+Z' | sort | tail -n 1)
if [ -n "$latest" ]; then
  evidence=$(
    git log --since="$latest" --format='%h %s' -- . ':!.engr' 2>/dev/null
    git status --porcelain -- . ':!.engr' 2>/dev/null | cut -c4- |
      while IFS= read -r path; do
        [ -e "$path" ] &&
          [ -n "$(find "$path" -maxdepth 0 -newermt "$latest" 2>/dev/null)" ] &&
          echo "$path (modified)"
      done
  )
else
  started=$(cat "${TMPDIR:-/tmp}/engr-heartbeat-${session:-unknown}.start" 2>/dev/null)
  evidence=$(
    git status --porcelain -- . ':!.engr' 2>/dev/null
    [ -n "$started" ] && git log --since="$started" --format='%h %s' -- . ':!.engr' 2>/dev/null
  )
fi
evidence=$(printf '%s\n' "$evidence" | sed '/^$/d' | head -n 8)
[ -n "$evidence" ] || exit 0

cat >&2 <<MESSAGE
engr: this edit is held. $count tool calls have gone by since engr was last written to, and this is unrecorded:
$evidence

Your session can end without warning, and the next one has only the repository and engr. Bring the handoff up to date, then retry the edit:
- no sidecar yet: engr work start <subject> --summary "..." and add the items you are working through
- a step finished: engr work item state/result (and item commit if it was committed)
- a decision settled or a question came up: record it or stage it in the backlog
- nothing else changed: engr work summary <subject> --text "where things stand, and what is next"
Any write to engr releases this.
MESSAGE
exit 2
