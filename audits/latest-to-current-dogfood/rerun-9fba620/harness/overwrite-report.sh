#!/bin/sh
# Round 31's P3 on the migration warning, with a real SIGKILL rather than the
# debug-only stop hook: the resume that publishes is not always the resume that
# reports. Publication destroys the comparison it is reporting on, so a resume
# interrupted between the overwrite and the report used to lose the warning for
# good — the next resume recomputes an empty list and completes in silence.
set -u
cd /audit/r6
CODE=J8SUZ4
AU=.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json

build() { # $1 = binary, $2 = workspace
  rm -rf "$2" && cp -a checkpoints/pre-confirm "$2"
  timeout -s KILL 0.960 "$1" --root "$2" confirm "CONFIRM $CODE" >/dev/null 2>&1
  [ -d "$2/.engr/local/migration/destination" ] || { echo "  not staged; widen"; return 1; }
  streams=$(ls "$2/.engr/eventstore/objects" 2>/dev/null | wc -l)
  [ "$streams" -ge 1 ] || { echo "  publication had not begun (streams=$streams); widen"; return 1; }
  sed -i 's/Migration continuity audit/Migration continuity audit (edited out of band)/' "$2/$AU"
  echo "  staged, streams=$streams, and one predecessor Object moved under it"
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
  for ms in 60 80 100 120 140 160 200 260 320 400; do
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
