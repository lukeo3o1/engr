#!/bin/sh
# Where exactly is the released build locked out? The barrier is what decides
# whether a crashed migration leaves a workspace the old tool can still write —
# which is what would make a stale stage dangerous rather than merely silent.
set -u
cd /audit/r4
CUR=/audit/bin/engr-current
OLD=/audit/bin/engr-latest
CODE=FBR8Z2
rm -rf bw && mkdir bw
for ms in 300 500 700 800 850 900 1000; do
  rm -rf bw/w && cp -a checkpoints/pre-confirm bw/w
  timeout -s KILL "$(awk -v m=$ms 'BEGIN{printf "%.3f", m/1000}')" $CUR --root bw/w confirm "CONFIRM $CODE" >/dev/null 2>&1
  fj=$([ -f bw/w/.engr/format.json ] && echo yes || echo no)
  st=$([ -d bw/w/.engr/local/migration/destination ] && echo yes || echo no)
  old=$($OLD --root bw/w ls --all 2>&1 | head -1 | cut -c1-70)
  oldcode=$($OLD --root bw/w ls --all >/dev/null 2>&1; echo $?)
  printf 'kill@%-5sms format.json=%-3s staged=%-3s  released build: exit=%-3s %s\n' "$ms" "$fj" "$st" "$oldcode" "$old"
  if [ "$fj" = yes ]; then printf '             format.json bytes: %s\n' "$(cat bw/w/.engr/format.json)"; fi
done
