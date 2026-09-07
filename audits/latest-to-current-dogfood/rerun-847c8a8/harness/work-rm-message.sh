#!/bin/sh
# `work rm` printed "no execution memory for backlog item <ref>" while removing
# execution memory that had two items, a summary and a commit. That is the
# wording a no-op would use. Ask whether the surface can tell the two apart:
# hash the sidecar either side, and run it again against nothing.
set -u
cd /audit/r8/project
E=/audit/bin/engr-current

$E backlog new --title "Work rm message probe" --text "A point so there is something to attach Work to." >/dev/null 2>&1
B=$($E backlog ls | sed -n 's/^\([0-9a-f]\{8\}\) .*Work rm message probe.*/\1/p' | head -1)
[ -n "$B" ] || { echo "REFUSING: no backlog topic to attach Work to"; exit 1; }
REF=$($E backlog show "$B" --format json | sed -n 's/.*"reference": *"\(engr:backlog:[a-z0-9]*\)".*/\1/p' | head -1)
U=$($E backlog show "$B" --format json | sed -n 's/.*"id": *"\([0-9a-f-]*\)".*/\1/p' | head -1)
SIDE=.engr/work/backlog/$U.json

$E work start "$REF" >/dev/null 2>&1
$E work item add "$REF" --text "An item, so the memory is not empty" >/dev/null 2>&1
$E work summary "$REF" --text "A summary, so the memory is plainly not empty" >/dev/null 2>&1

echo "--- the execution memory that exists before the removal ---"
if [ -f "$SIDE" ]; then
  before=$(sha256sum "$SIDE" | cut -d' ' -f1)
  echo "  sidecar $SIDE"
  echo "  sha256  $before"
  cat "$SIDE"; echo
else
  echo "  REFUSING: no sidecar was written, so there is nothing to measure"; exit 1
fi

echo
echo "--- work rm, against memory that exists ---"
out1=$($E work rm "$REF" 2>&1); rc1=$?
printf '  said : %s\n' "$out1"
printf '  exit : %s\n' "$rc1"
after=no; [ -f "$SIDE" ] && after=yes
printf '  sidecar still present: %s\n' "$after"

echo
echo "--- work rm again, against nothing ---"
out2=$($E work rm "$REF" 2>&1); rc2=$?
printf '  said : %s\n' "$out2"
printf '  exit : %s\n' "$rc2"

echo
echo "--- can a reader tell the two apart? ---"
if [ "$out1" = "$out2" ] && [ "$rc1" = "$rc2" ]; then
  echo "  NO: removing real execution memory and removing nothing are byte-identical,"
  echo "      same wording and same exit $rc1."
else
  echo "  YES: they differ."
  echo "      removed something -> exit $rc1: $out1"
  echo "      removed nothing   -> exit $rc2: $out2"
fi
