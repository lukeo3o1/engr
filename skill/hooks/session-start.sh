#!/bin/sh
# Claude Code SessionStart hook: put what the last session left into this one.
#
# A context that has just been compacted or cleared is exactly the one that no
# longer remembers to run the startup reads, and an agent that skips them
# re-derives, or re-decides, what engr already holds. What this prints on stdout
# is added to the new context. It writes nothing.
cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
[ -d .engr ] || exit 0
command -v engr >/dev/null 2>&1 || exit 0

run() {
  printf '\n$ engr %s\n' "$*"
  engr "$@" 2>&1 | head -n 80
}

echo "engr: what the previous session left. Read it before acting, and check"
echo "each sidecar claim against the thing it names before relying on it."
run ls --verify
run backlog ls
run candidate
run work ls

# The sidecar is the handoff itself, so it is shown in full rather than as a
# row. `work ls` prints a short id, and a Backlog subject has to be written as
# its full reference, so that is looked up rather than built.
engr work ls 2>/dev/null | while read -r id kind _; do
  case "$kind" in
    obj | object) run work show "$id" ;;
    backlog)
      reference=$(engr backlog show "$id" --format json 2>/dev/null |
        sed -n 's/^  "reference": "\(engr:backlog:[^"]*\)".*/\1/p')
      [ -n "$reference" ] && run work show "$reference" ;;
  esac
done

# Work nobody recorded: the repository moved after the newest sidecar was last
# written. A timestamp is all `work ls` offers, so commit dates are compared
# against it, and uncommitted files by modification time.
latest=$(engr work ls 2>/dev/null |
  grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]+Z' | sort | tail -n 1)
if [ -n "$latest" ]; then
  commits=$(git log --since="$latest" --format='  %h %s' -- . ':!.engr' 2>/dev/null)
  if [ -n "$commits" ]; then
    echo
    echo "Committed after the newest sidecar was written ($latest):"
    echo "$commits" | head -n 20
  fi
fi
dirty=$(git status --porcelain -- . ':!.engr' 2>/dev/null)
if [ -n "$dirty" ]; then
  echo
  echo "Uncommitted outside .engr:"
  echo "$dirty" | head -n 20
fi
exit 0
