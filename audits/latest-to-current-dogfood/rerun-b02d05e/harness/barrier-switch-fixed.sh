#!/bin/sh
# The same measurement as barrier-switch.sh, against the fixed build, printing
# the whole of what the resume says rather than its first line — because the
# fix is a line the first one does not carry.
set -u
cd /audit/r4
CUR=/audit/bin/engr-fixed
CODE=FBR8Z2
AU=.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
rm -rf bsf && mkdir bsf

try() {
  label=$1; ms=$2
  rm -rf bsf/w && cp -a checkpoints/pre-confirm bsf/w
  timeout -s KILL "$(awk -v m=$ms 'BEGIN{printf "%.3f", m/1000}')" $CUR --root bsf/w confirm "CONFIRM $CODE" >/dev/null 2>&1
  staged=$([ -d bsf/w/.engr/local/migration/destination ] && echo yes || echo no)
  streams=$(ls bsf/w/.engr/eventstore/objects 2>/dev/null | wc -l)
  barrier=$(grep -q 'migration-in-progress' bsf/w/.engr/format.json 2>/dev/null && echo installed || echo "not yet")
  [ "$staged" = yes ] || { printf '%-18s kill@%-5s staged=no\n' "$label" "$ms"; return; }
  sed -i 's/Migration continuity audit/Migration continuity audit (edited out of band)/' "bsf/w/$AU"
  out=$($CUR --root bsf/w confirm "CONFIRM $CODE" 2>&1); code=$?
  kept=$(grep -c 'edited out of band' "bsf/w/$AU" 2>/dev/null || echo 0)
  printf '%-18s kill@%-5s barrier=%-9s streams=%s/3  exit=%-3s edit_survived=%s\n' "$label" "$ms" "$barrier" "$streams" "$code" "$kept"
  printf '%s\n' "$out" | sed 's/^/    /'
}

for ms in 780 800 820 840 860 880 900 920 940 960; do try instant "$ms"; done
