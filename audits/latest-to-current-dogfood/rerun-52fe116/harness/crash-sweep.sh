#!/bin/sh
set -u
cd /audit/r2
CODE=AMKRH4
rm -rf crash-runs && mkdir crash-runs
cp -a checkpoints/pre-confirm crash-runs/baseline
start=$(date +%s%N)
/audit/bin/engr-current --root crash-runs/baseline confirm "CONFIRM $CODE" >crash-runs/baseline.out 2>&1
end=$(date +%s%N)
echo "uninterrupted confirm: $(( (end-start)/1000000 )) ms"
cat crash-runs/baseline.out
echo "---- SIGKILL sweep across that window ----"
for ms in "$@"; do
  run="crash-runs/t$ms"
  cp -a checkpoints/pre-confirm "$run"
  secs=$(awk -v m="$ms" 'BEGIN{printf "%.3f", m/1000}')
  timeout -s KILL "$secs" /audit/bin/engr-current --root "$run" confirm "CONFIRM $CODE" >"$run.out" 2>"$run.err"
  code=$?
  version=no; [ -f "$run/.engr/VERSION" ] && version=yes
  pred=no;    [ -f "$run/.engr/format.json" ] && pred=yes
  dest=no;    [ -d "$run/.engr/local/migration/destination" ] && dest=yes
  cand=no;    [ -f "$run/.engr/local/challenges/$CODE.json" ] && cand=yes
  newev=$(ls "$run/.engr/eventstore/objects" 2>/dev/null | wc -l)
  oldev=$(ls "$run/.engr/events" 2>/dev/null | wc -l)
  printf 't%-4sms exit=%-4s VERSION=%-3s format.json=%-3s staged_dest=%-3s challenge=%-3s new_eventstore=%s old_events=%s\n' \
    "$ms" "$code" "$version" "$pred" "$dest" "$cand" "$newev" "$oldev"
done
