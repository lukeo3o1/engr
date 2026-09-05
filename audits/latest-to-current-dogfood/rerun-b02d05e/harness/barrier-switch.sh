#!/bin/sh
# The decisive measurement: the same out-of-band edit, at two staged instants
# that differ only by whether the barrier has been installed.
#
# If it is refused before the barrier and published after it, then what decides
# whether the workspace is re-checked is the barrier — and the barrier only
# establishes that the *released build* cannot have written. Anything else still
# can.
set -u
cd /audit/r4
CUR=/audit/bin/engr-current
OLD=/audit/bin/engr-latest
CODE=FBR8Z2
AU=.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
rm -rf bs && mkdir bs

try() { # label, kill-ms
  label=$1; ms=$2
  rm -rf bs/w && cp -a checkpoints/pre-confirm bs/w
  timeout -s KILL "$(awk -v m=$ms 'BEGIN{printf "%.3f", m/1000}')" $CUR --root bs/w confirm "CONFIRM $CODE" >/dev/null 2>&1
  staged=$([ -d bs/w/.engr/local/migration/destination ] && echo yes || echo no)
  barrier=$(grep -q 'migration-in-progress' bs/w/.engr/format.json 2>/dev/null && echo installed || echo "not yet")
  [ "$staged" = yes ] || { printf '%-22s kill@%-5s staged=no  — not in the window this time\n' "$label" "$ms"; return; }
  sed -i 's/Migration continuity audit/Migration continuity audit (edited out of band)/' "bs/w/$AU"
  out=$($CUR --root bs/w confirm "CONFIRM $CODE" 2>&1); code=$?
  kept=$(grep -c 'edited out of band' "bs/w/$AU" 2>/dev/null || echo 0)
  printf '%-22s kill@%-5s staged=yes barrier=%-9s confirm exit=%-3s edit_survived=%s\n' "$label" "$ms" "$barrier" "$code" "$kept"
  printf '                       %s\n' "$(printf '%s' "$out" | head -1 | cut -c1-150)"
}

for ms in 790 800 810 820; do try "before the barrier" "$ms"; done
for ms in 860 880 900 950; do try "after the barrier"  "$ms"; done
