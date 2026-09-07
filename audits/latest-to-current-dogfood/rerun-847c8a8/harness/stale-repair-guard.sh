#!/bin/sh
# The guard that must hold: once the Object has moved on, a repair code prepared
# against the older revision must be refused. Two codes are prepared against one
# damaged Object; confirming the first moves it to rev 2, and the second is then
# a code for a predecessor that no longer stands.
set -u
cd /audit/r8
C=/audit/bin/engr-current
AU=.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
S=.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl
code_of() { printf '%s' "$1" | sed -n 's/.*CONFIRM \([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]\{6\}\).*/\1/p' | head -1; }

rm -rf srg && cp -a st-divergent srg
a=$(code_of "$($C --root srg repair 01a05e55-74 2>&1)")
b=$(code_of "$($C --root srg repair 01a05e55-74 2>&1)")
echo "two codes prepared: $a and $b"
echo "challenges on disk: $(ls srg/.engr/local/challenges 2>/dev/null | tr '\n' ' ')"
echo
echo "--- confirm the first ---"
__out=$($C --root srg confirm "CONFIRM $a" 2>&1); __rc=$?
printf '%s\n' "$__out" | head -1; echo "  exit=$__rc"
echo "  rev now: $(sed -n 's/.*"rev":\([0-9]*\).*/\1/p' "srg/$AU" | head -1), events $(wc -l < "srg/$S")"
echo
echo "--- and the second, prepared against the revision that is gone ---"
__out=$($C --root srg confirm "CONFIRM $b" 2>&1); __rc=$?
printf '%s\n' "$__out" | head -1; echo "  exit=$__rc"
echo "  rev now: $(sed -n 's/.*"rev":\([0-9]*\).*/\1/p' "srg/$AU" | head -1), events $(wc -l < "srg/$S")"
echo
echo "--- and an ordinary admitted mutation between prepare and confirm ---"
rm -rf srg2 && cp -a st-divergent srg2
c=$(code_of "$($C --root srg2 repair 01a05e55-74 2>&1)")
echo "repair code prepared: $c"
$C --root srg2 confirm "CONFIRM $c" >/dev/null 2>&1
d=$(code_of "$($C --root srg2 prepare --object 01a05e55-74 --add --no-based-on --text 'Ordinary work admitted after the repair.' 2>&1)")
$C --root srg2 confirm "CONFIRM $d" 2>&1 | head -1
echo "  rev now: $(sed -n 's/.*"rev":\([0-9]*\).*/\1/p' "srg2/$AU" | head -1)"
echo "--- retyping the spent repair code against the moved record ---"
__out=$($C --root srg2 confirm "CONFIRM $c" 2>&1); __rc=$?
printf '%s\n' "$__out" | head -1; echo "  exit=$__rc"
$C --root srg2 verify >/dev/null 2>&1; echo "  verify exit=$?"
