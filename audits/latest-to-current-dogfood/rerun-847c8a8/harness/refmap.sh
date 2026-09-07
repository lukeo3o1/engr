#!/bin/sh
# Which compact reference belongs to which object, so a hardcoded `engr:obj:...`
# in the harness can be checked against the workspace instead of trusted.
set -u
E=/audit/bin/engr-current
cd "${1:-/audit/r8/project}"
echo "--- workspace: $(pwd) ---"
$E ls --all 2>/dev/null
echo
for o in $($E ls --all 2>/dev/null | awk '{print $1}'); do
  printf '%-14s ' "$o"
  $E show "$o" --format json 2>/dev/null \
    | sed -n 's/.*"reference": *"\(engr:obj:[a-z0-9]*\)".*/\1/p' | head -1
done

echo
echo "--- the compact references the harness hardcodes, checked against this workspace ---"
# Three scripts name 01a05e55-74 by its compact reference (backlog.ps1,
# expect-messages.sh, work-empty-lists.sh). It is a migrated fixture object, so
# its id should be identical every run -- but that is a claim about the code,
# not a licence to skip asking.
check() { # $1 = object, $2 = the reference the harness assumes, $3 = who assumes it
  got=$($E show "$1" --format json 2>/dev/null \
        | sed -n 's/.*"reference": *"\(engr:obj:[a-z0-9]*\)".*/\1/p' | head -1)
  if [ "$got" = "$2" ]; then
    printf '  OK       %s = %s  (%s)\n' "$1" "$2" "$3"
  else
    printf '  STALE    %s is %s, not %s  -- fix %s\n' "$1" "$got" "$2" "$3"
  fi
}
check 01a05e55-74 engr:obj:01m1f5ax7bedrbew8012yq3ma9 "backlog.ps1, expect-messages.sh, work-empty-lists.sh"
check 01a05e55-e6 engr:obj:01m1f5bshbeq1rpckc726yrk2b "collection.sh's B, now resolved at runtime"
