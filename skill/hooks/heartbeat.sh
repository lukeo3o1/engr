#!/bin/sh
# Claude Code PostToolUse hook: remind, in the middle of the work, that the
# handoff is falling behind it.
#
# Guidance to write "at the moment something settles" held for the first stretch
# of a session and then stopped: once an agent is deep in a build-and-fix loop it
# does not stop to write, and a turn limit or a cleared context fires no Stop
# hook to catch it. So this counts tool calls since engr was last written to and
# speaks every HEARTBEAT_EVERY of them, and straight after a git commit, which is
# the moment an item is most likely to have finished. It changes nothing; what
# it prints is added to the agent's context.
input=$(cat)
cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
[ -d .engr ] || exit 0

every=${HEARTBEAT_EVERY:-10}
session=$(printf '%s' "$input" | sed -n 's/.*"session_id": *"\([^"]*\)".*/\1/p' | head -n 1)
counter="${TMPDIR:-/tmp}/engr-heartbeat-${session:-unknown}"
# When this session began, for gate.sh: with no sidecar yet, a commit made since
# then is still work nobody recorded.
[ -e "$counter.start" ] || date -u +%Y-%m-%dT%H:%M:%SZ > "$counter.start"

say() {
  printf '{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"%s"}}\n' "$1"
}

# A write to engr's agent-managed state, or an admission, resets the count.
if printf '%s' "$input" | grep -qE 'engr (work (start|summary|item|block|unblock|depend|undepend)|backlog (new|add|revise|merge|produced|consume|rename|subjects)|prepare|collection (new|add|order|priority|rm|state))'; then
  echo 0 > "$counter"
  exit 0
fi

count=$(( $(cat "$counter" 2>/dev/null || echo 0) + 1 ))
echo "$count" > "$counter"

# A commit is not a handoff, so it does not reset the count gate.sh reads; it
# is only the likeliest moment for an item to have finished.
if printf '%s' "$input" | grep -qE '"command": *"[^"]*git commit'; then
  say "engr: a commit just landed. If it finishes a work item, close it now, before anything else: item state done, item result saying how you know, item commit --commit HEAD. If it settles a decision or leaves a question open, record that too."
  exit 0
fi

# Speaking does not reset the count either: a reminder that was read past is
# exactly the case gate.sh exists for, and it can only see that if the count
# keeps growing until engr is actually written to.
if [ $((count % every)) -eq 0 ]; then
  say "engr: $count tool calls since execution memory was last written. Your session can end without warning, and the next one sees only the repository and engr. If since then a step finished, a hypothesis was confirmed or ruled out, a decision settled or a question came up, record it now. If nothing did, carry on."
fi
exit 0
