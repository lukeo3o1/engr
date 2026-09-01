#!/bin/sh
set -u
cd /audit
rm -rf crash-runs && mkdir crash-runs
cp -a checkpoints/crash-a crash-runs/baseline
start=$(date +%s%N)
/audit/bin/engr-current --root crash-runs/baseline confirm "CONFIRM 8DZM7N" >crash-runs/baseline.out 2>&1
end=$(date +%s%N)
echo "uninterrupted confirm: $(( (end-start)/1000000 )) ms"
echo "---- SIGKILL sweep across that window ----"
for ms in 300 400 500 550 600 620 640 660 680 700 720 740 760 780 800 900; do
  run="crash-runs/t$ms"
  cp -a checkpoints/crash-a "$run"
  secs=$(awk -v m="$ms" 'BEGIN{printf "%.3f", m/1000}')
  timeout -s KILL "$secs" /audit/bin/engr-current --root "$run" confirm "CONFIRM 8DZM7N" >"$run.out" 2>"$run.err"
  code=$?
  version=no; [ -f "$run/.engr/VERSION" ] && version=yes
  pred=no;    [ -f "$run/.engr/format.json" ] && pred=yes
  dest=no;    [ -d "$run/.engr/local/migration/destination" ] && dest=yes
  newev=$(ls "$run/.engr/eventstore/objects" 2>/dev/null | wc -l)
  newob=$(grep -l 'object.migrated' "$run"/.engr/objects/*.json 2>/dev/null | wc -l)
  migob=$(grep -lc '"rev":1,' "$run"/.engr/objects/*.json 2>/dev/null | wc -l)
  oldev=$(ls "$run/.engr/events" 2>/dev/null | wc -l)
  printf 't%-4sms exit=%-4s VERSION=%-3s format.json=%-3s staged_dest=%-3s new_eventstore=%s migrated_objects=%s old_events=%s\n' \
    "$ms" "$code" "$version" "$pred" "$dest" "$newev" "$migob" "$oldev"
done
