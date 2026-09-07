#!/bin/sh
# Round 31's P3 on the migration warning, with a real SIGKILL rather than the
# debug-only stop hook: the resume that publishes is not always the resume that
# reports. Publication destroys the comparison it is reporting on, so a resume
# interrupted between the overwrite and the report used to lose the warning for
# good — the next resume recomputes an empty list and completes in silence.
set -u
cd /audit/r8
CODE=XDKEFZ
AU=.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json

# The instant this stops at moves with the host and the day, so it is swept
# rather than pinned: the first kill that leaves a staged destination with at
# least one Event stream published is the one this probe needs. r6's fixed
# 960 ms found nothing on this host, where publication begins nearer 1000 ms.
build() { # $1 = binary, $2 = workspace
  for at in 0.960 1.000 1.020 1.040 1.060 1.080 1.100; do
    rm -rf "$2" && cp -a checkpoints/pre-confirm "$2"
    timeout -s KILL "$at" "$1" --root "$2" confirm "CONFIRM $CODE" >/dev/null 2>&1
    [ -d "$2/.engr/local/migration/destination" ] || continue
    streams=$(ls "$2/.engr/eventstore/objects" 2>/dev/null | wc -l)
    [ "$streams" -ge 1 ] && break
  done
  [ -d "$2/.engr/local/migration/destination" ] || { echo "  never staged; widen"; return 1; }
  streams=$(ls "$2/.engr/eventstore/objects" 2>/dev/null | wc -l)
  [ "$streams" -ge 1 ] || { echo "  publication never began (streams=$streams); widen"; return 1; }
  sed -i 's/Migration continuity audit/Migration continuity audit (edited out of band)/' "$2/$AU"
  echo "  staged by a kill at ${at}s, streams=$streams, and one predecessor Object moved under it"
  return 0
}

echo "===== how long an uninterrupted resume takes ====="
build /audit/bin/engr-current or || exit 1
start=$(date +%s%N)
/audit/bin/engr-current --root or confirm "CONFIRM $CODE" >or.out 2>&1
end=$(date +%s%N)
echo "  resume: $(( (end-start)/1000000 )) ms"
cat or.out

for bin in prev current; do
  echo
  echo "===== $bin: the resume that publishes is killed before it can report ====="
  found=no
  for ms in 60 80 100 120 140 160 200 260 320 400 500 600; do
    build "/audit/bin/engr-$bin" ov >/dev/null 2>&1 || continue
    timeout -s KILL "$(awk -v m=$ms 'BEGIN{printf "%.3f", m/1000}')" "/audit/bin/engr-$bin" --root ov confirm "CONFIRM $CODE" >/dev/null 2>&1
    if ! grep -q 'edited out of band' "ov/$AU" 2>/dev/null; then
      version=$([ -f ov/.engr/VERSION ] && echo yes || echo no)
      echo "  killed at ${ms}ms: the source was overwritten, VERSION=$version"
      found=$ms; break
    fi
  done
  [ "$found" = no ] && { echo "  never landed after the overwrite; widen the sweep"; continue; }
  echo "  -- and what the next resume says --"
  "/audit/bin/engr-$bin" --root ov confirm "CONFIRM $CODE" 2>&1
  echo "  exit=$?"
done
