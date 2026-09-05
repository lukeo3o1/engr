#!/bin/sh
# Is the orphan stage — a destination with no manifest — reachable by a real
# interruption rather than by hand? A withdrawal only happens when a *new*
# migration supersedes a staged one, so: crash a confirm after the destination
# is staged, run `migrate` again, and kill that at a sweep of instants.
set -u
cd /audit/r4
E=/audit/bin/engr-current
CODE=FBR8Z2
rm -rf wi && mkdir wi

echo "===== a real crash with the destination staged ====="
cp -a checkpoints/pre-confirm wi/w
timeout -s KILL 0.850 $E --root wi/w confirm "CONFIRM $CODE" >/dev/null 2>&1
echo "VERSION=$([ -f wi/w/.engr/VERSION ] && echo yes || echo no)  staged_dest=$([ -d wi/w/.engr/local/migration/destination ] && echo yes || echo no)  manifest=$([ -f wi/w/.engr/local/migration/manifest.json ] && echo yes || echo no)  challenge=$([ -f wi/w/.engr/local/challenges/$CODE.json ] && echo yes || echo no)"
echo "-- what a read says --"
$E --root wi/w ls 2>&1 | head -1
echo "-- and what `migrate` does with a staged plan --"
$E --root wi/w migrate 2>&1 | tail -2; echo "migrate exit=$?"

echo
echo "===== now interrupt that same migrate across a sweep ====="
for ms in 50 100 150 200 300 400; do
  rm -rf wi/k && cp -a wi/w wi/k
  timeout -s KILL "$(awk -v m=$ms 'BEGIN{printf "%.3f", m/1000}')" $E --root wi/k migrate >/dev/null 2>&1
  d=$([ -d wi/k/.engr/local/migration/destination ] && echo yes || echo no)
  m=$([ -f wi/k/.engr/local/migration/manifest.json ] && echo yes || echo no)
  dir=$([ -d wi/k/.engr/local/migration ] && echo yes || echo no)
  lsout=$($E --root wi/k ls 2>&1 | head -1)
  mgout=$($E --root wi/k migrate 2>&1 | head -1)
  printf 'kill@%-4sms  migration_dir=%-3s manifest=%-3s destination=%-3s\n' "$ms" "$dir" "$m" "$d"
  printf '             ls     : %s\n' "$(printf '%s' "$lsout" | cut -c1-110)"
  printf '             migrate: %s\n' "$(printf '%s' "$mgout" | cut -c1-110)"
done
