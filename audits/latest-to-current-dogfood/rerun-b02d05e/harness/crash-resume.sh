#!/bin/sh
set -u
cd /audit/r4
E=/audit/bin/engr-current
CODE=FBR8Z2
inv() { (cd "$1" && find .engr -type f -not -path '.engr/local/*' | LC_ALL=C sort | xargs sha256sum 2>/dev/null | sha256sum | cut -d' ' -f1); }
evat() { cat "$1"/.engr/eventstore/objects/*.jsonl 2>/dev/null | sed -n 's/.*"admitted":{"at":"\([^"]*\)","by":"human","confirmation".*/\1/p' | LC_ALL=C sort -u | tr '\n' ' '; }

echo "===== baseline (uninterrupted) ====="
echo "inventory: $(inv crash-runs/baseline)"
echo "event admitted.at: $(evat crash-runs/baseline)"
echo "events per stream: $(for f in crash-runs/baseline/.engr/eventstore/objects/*.jsonl; do wc -l < "$f"; done | tr '\n' ' ')"

for ms in "$@"; do
  run="crash-runs/t$ms"
  [ -d "$run" ] || continue
  echo
  echo "===== interrupted at ${ms}ms ====="
  echo "-- while incomplete, does a read surface fail closed? --"
  $E --root "$run" ls >/tmp/o 2>/tmp/e; echo "ls exit=$? : $(head -1 /tmp/e)$(head -1 /tmp/o)"
  echo "-- resume: same code, again --"
  $E --root "$run" confirm "CONFIRM $CODE" >/tmp/o 2>/tmp/e; echo "confirm exit=$? : $(head -1 /tmp/o)$(head -1 /tmp/e)"
  echo "-- resume again (idempotence) --"
  $E --root "$run" confirm "CONFIRM $CODE" >/tmp/o 2>/tmp/e; echo "confirm exit=$? : $(head -1 /tmp/o)$(head -1 /tmp/e)"
  echo "inventory: $(inv "$run")"
  echo "event admitted.at: $(evat "$run")"
  echo "events per stream: $(for f in "$run"/.engr/eventstore/objects/*.jsonl; do wc -l < "$f"; done | tr '\n' ' ')"
  echo "-- verify --"
  $E --root "$run" verify >/tmp/o 2>/tmp/e; echo "verify exit=$? : $(grep -c PASS /tmp/o) PASS $(head -1 /tmp/e)"
done
