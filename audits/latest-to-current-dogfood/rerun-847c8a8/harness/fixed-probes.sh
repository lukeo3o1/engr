#!/bin/sh
# The two findings, re-asked of the fixed build, with the head that failed them
# beside it. Every probe here is one this run already ran and recorded.
set -u
cd /audit/r8
F=/audit/bin/engr-current
C=/audit/bin/engr-r33
# A comparison probe whose two binaries are the same measures nothing and
# reports green. r8's `engr-fixed` and `engr-current` are byte-identical, and
# this probe used to name both. Hash either side and refuse.
if [ "$(sha256sum "$F" | cut -d' ' -f1)" = "$(sha256sum "$C" | cut -d' ' -f1)" ]; then
  echo "REFUSING: \$F and \$C are the same binary -- this probe would measure nothing"
  exit 1
fi
code_of() { printf '%s' "$1" | sed -n 's/.*CONFIRM \([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]\{6\}\).*/\1/p' | head -1; }

echo "############ the binary is the one intended ############"
printf 'round 35 sentence, head under test: '
$F protocol 2>/dev/null | grep -c 'There are two crash windows'

echo
echo "############ F-1: which screens name both revisions ############"
for pair in "st-missing absent" "st-divergent divergent" "st-tampered tampered"; do
  set -- $pair
  ws=$1; what=$2
  rm -rf fp && cp -a "$ws" fp
  now=$($F --root fp repair 01a05e55-74 2>&1 | grep -c 'at rev 1, admitted as rev 2')
  rm -rf fp && cp -a "$ws" fp
  was=$($C --root fp repair 01a05e55-74 2>&1 | grep -c 'at rev 1, admitted as rev 2')
  printf '  %-10s fixed=%s  r33=%s\n' "$what" "$now" "$was"
done
echo "  -- and the whole fixed screen for the divergent case --"
rm -rf fp && cp -a st-divergent fp
$F --root fp repair 01a05e55-74 2>&1 | sed 's/^/    /'
echo "  -- and the same screen re-rendered from the pending code --"
rm -rf fp && cp -a st-divergent fp
out=$($F --root fp repair 01a05e55-74 2>&1)
$F --root fp candidate "$(code_of "$out")" 2>&1 | sed 's/^/    /'

echo
echo "############ F-2: a code retyped after the damage is gone ############"
for pair in "st-missing absent" "st-divergent divergent" "st-tampered tampered"; do
  set -- $pair
  ws=$1; what=$2
  for bin in "$F fixed" "$C r33"; do
    set -- $bin
    b=$1; who=$2
    rm -rf f2p && cp -a "$ws" f2p
    T=f2p/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
    out=$($b --root f2p repair 01a05e55-74 2>&1)
    c=$(code_of "$out")
    [ -n "$c" ] || { printf '  %-10s %-8s no code offered\n' "$what" "$who"; continue; }
    cp checkpoints/migrated/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json "$T"
    screen=$($b --root f2p candidate "$c" 2>&1)
    offers=$(printf '%s' "$screen" | grep -c 'Type this exactly to confirm')
    said=$(printf '%s' "$screen" | grep -c 'nothing here needs confirming')
    ev_before=$(wc -l < f2p/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl)
    $b --root f2p confirm "CONFIRM $c" >/dev/null 2>&1; rc=$?
    ev_after=$(wc -l < f2p/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl)
    printf '  %-10s %-8s screen offers code=%s says-settled=%s  confirm exit=%-3s events %s -> %s\n' \
      "$what" "$who" "$offers" "$said" "$rc" "$ev_before" "$ev_after"
  done
done
echo "  -- the fixed screen for that state, whole --"
rm -rf f2p && cp -a st-divergent f2p
T=f2p/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
out=$($F --root f2p repair 01a05e55-74 2>&1)
c=$(code_of "$out")
cp checkpoints/migrated/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json "$T"
$F --root f2p candidate "$c" 2>&1 | sed 's/^/    /'
echo "  -- and confirming it --"
$F --root f2p confirm "CONFIRM $c" 2>&1 | sed 's/^/    /'

echo
echo "############ and a repair that is still needed still works ############"
rm -rf f3p && cp -a st-missing f3p
out=$($F --root f3p repair 01a05e55-74 2>&1)
printf '%s\n' "$out" | head -4 | sed 's/^/    /'
$F --root f3p confirm "CONFIRM $(code_of "$out")" 2>&1 | head -1 | sed 's/^/    /'
$F --root f3p verify >/dev/null 2>&1; echo "    verify exit=$?"
sed -n 's/.*"rev":\([0-9]*\).*"type":"\([a-z._0-9]*\)".*/    rev=\1 type=\2/p' \
  f3p/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl
