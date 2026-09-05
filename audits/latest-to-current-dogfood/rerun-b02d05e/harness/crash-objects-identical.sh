#!/bin/sh
# The claim the inventories cannot make on their own: every resumed migration
# produced the *same record*. Event ids and admitted.at differ per run by
# construction, so the comparison is over objects/ alone, plus the event count
# and the event type per stream.
set -u
cd /audit/r4
oh() { (cd "$1" && find .engr/objects -type f | LC_ALL=C sort | xargs sha256sum | sha256sum | cut -d' ' -f1); }
base=$(oh crash-runs/baseline)
echo "baseline objects/ : $base"
for d in crash-runs/t*/; do
  [ -d "$d" ] || continue
  h=$(oh "$d")
  n=$(cat "$d".engr/eventstore/objects/*.jsonl | wc -l)
  t=$(cat "$d".engr/eventstore/objects/*.jsonl | sed -n 's/.*"type":"\([a-z._0-9]*\)".*/\1/p' | sort -u | tr '\n' ' ')
  printf '%-22s %s  %s  events=%s types=%s\n' "$d" "$h" "$([ "$h" = "$base" ] && echo IDENTICAL || echo DIFFERENT)" "$n" "$t"
done
echo
echo "and the carried-forward workspace this audit is using:"
printf '%-22s %s  %s\n' project "$(oh project)" "$([ "$(oh project)" = "$base" ] && echo IDENTICAL || echo DIFFERENT)"
