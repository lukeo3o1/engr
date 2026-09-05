#!/bin/sh
# The real withdrawal: a staged plan whose source has moved under it. The plan
# pins every predecessor file by digest, so the stage cannot be resumed and has
# to be withdrawn and prepared again — which is the operation round 25 found
# could be interrupted into a wedge.
set -u
cd /audit/r4
E=/audit/bin/engr-current
CODE=FBR8Z2
rm -rf ws && mkdir ws
cp -a wi/w ws/w        # crashed confirm: manifest + destination + live challenge
echo "staged: manifest=$([ -f ws/w/.engr/local/migration/manifest.json ] && echo yes) destination=$([ -d ws/w/.engr/local/migration/destination ] && echo yes) challenge=$([ -f ws/w/.engr/local/challenges/$CODE.json ] && echo yes)"

echo
echo "===== move the source under the staged plan ====="
sed -i 's/Migration continuity audit/Migration continuity audit (edited under the plan)/' ws/w/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
echo "-- resuming the staged plan --"
$E --root ws/w confirm "CONFIRM $CODE" 2>&1 | head -2; echo "confirm exit=$?"
echo "-- and preparing again --"
$E --root ws/w migrate 2>&1 | tail -3; echo "migrate exit=$?"
echo "after: manifest=$([ -f ws/w/.engr/local/migration/manifest.json ] && echo yes || echo no) destination=$([ -d ws/w/.engr/local/migration/destination ] && echo yes || echo no) challenges=$(ls ws/w/.engr/local/challenges 2>/dev/null | tr '\n' ' ')"

echo
echo "===== interrupt that withdrawal across a sweep ====="
for ms in 20 40 60 80 120 200; do
  rm -rf ws/k && cp -a wi/w ws/k
  sed -i 's/Migration continuity audit/Migration continuity audit (edited under the plan)/' ws/k/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
  timeout -s KILL "$(awk -v m=$ms 'BEGIN{printf "%.3f", m/1000}')" $E --root ws/k migrate >/dev/null 2>&1
  m=$([ -f ws/k/.engr/local/migration/manifest.json ] && echo yes || echo no)
  d=$([ -d ws/k/.engr/local/migration/destination ] && echo yes || echo no)
  c=$(ls ws/k/.engr/local/challenges 2>/dev/null | wc -l)
  printf 'kill@%-4sms manifest=%-3s destination=%-3s challenges=%s\n' "$ms" "$m" "$d" "$c"
  printf '            ls     : %s\n' "$($E --root ws/k ls 2>&1 | head -1 | cut -c1-110)"
  printf '            migrate: %s\n' "$($E --root ws/k migrate 2>&1 | head -1 | cut -c1-110)"
done
